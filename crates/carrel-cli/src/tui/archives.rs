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
            if let Ok(previous) = std::fs::canonicalize(self.archive.root()) {
                known.retain(|r| *r != previous);
                known.insert(0, previous);
            }
            known.retain(|r| *r != root);
            known.insert(0, root);
            app.known_archives = known;
        }
        app.here = self.here.take();
        app.service = self.service.clone();
        app.previews = super::preview::Previews::with(self.previews.protocol);
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

#[cfg(test)]
mod tests {
    use carrel_core::archive::{Archive, InitOptions};
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::super::app::{App, Effect, Preferences, Tab};
    use super::super::embedding::Here;
    use super::super::review::Purpose;
    use super::super::screens::InputPurpose;
    use super::super::tests::{key, sample, screen};

    fn ctrl_o() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)
    }

    fn typed(app: &mut App, text: &str) -> Option<Effect> {
        for c in text.chars() {
            app.key(key(KeyCode::Char(c)));
        }
        app.key(key(KeyCode::Enter))
    }

    /// A second archive, called Other, beside the sample's.
    fn other(dir: &std::path::Path) -> std::path::PathBuf {
        let root = dir.join("other");
        Archive::init(&root, &InitOptions::new("Other")).unwrap();
        std::fs::canonicalize(root).unwrap()
    }

    #[test]
    fn ctrl_o_switches_to_a_recent_archive_or_one_by_its_path() {
        let (dir, mut app) = sample(false);
        assert_eq!(app.preferences, Preferences::InMemory);
        let sample_root = std::fs::canonicalize(app.archive.root()).unwrap();
        let other = other(dir.path());
        app.known_archives = vec![sample_root.clone(), other.clone()];

        // On any screen: the recent archives other than this one, a path, or a new one.
        app.key(key(KeyCode::Char('6')));
        app.key(ctrl_o());
        let picker = app.picker.clone().unwrap();
        assert!(matches!(picker.purpose, Purpose::Archive(_)));
        assert_eq!(
            picker.collections,
            vec![
                format!("Other  {}", other.display()),
                "Open an archive by its path…".to_owned(),
                "Create a new archive…".to_owned(),
            ]
        );
        let text = screen(&app, 120, 30);
        assert!(text.contains(" Archives (type to narrow) "), "{text}");
        let effect = app.key(key(KeyCode::Enter));
        assert_eq!(effect, Some(Effect::OpenArchive(other.clone())));

        // Switching shows the other archive, on the same screen, and remembers this one.
        let message = app.switch_archive(&other).unwrap();
        assert_eq!(message, "Opened “Other”");
        assert_eq!(app.archive.doc().name, "Other");
        assert_eq!(app.tab, Tab::History);
        assert_eq!(app.preferences, Preferences::InMemory);
        assert_eq!(app.known_archives, vec![other.clone(), sample_root.clone()]);
        let text = screen(&app, 120, 30);
        assert!(text.contains(" Other "), "{text}");

        // Back by its path: any folder inside it will do.
        app.key(ctrl_o());
        app.picker.as_mut().unwrap().filter = "by its path".into();
        app.key(key(KeyCode::Enter));
        assert_eq!(
            app.input.as_ref().map(|i| i.purpose.clone()),
            Some(InputPurpose::OpenArchive)
        );
        let inside = sample_root.join("Projects");
        let effect = typed(&mut app, &format!("'{}'", inside.display()));
        assert_eq!(effect, Some(Effect::OpenArchive(inside.clone())));
        app.switch_archive(&inside).unwrap();
        assert_eq!(app.archive.doc().name, "Sample archive");

        // A folder that isn't an archive says so, and this one stays.
        let error = app.switch_archive(dir.path()).unwrap_err();
        assert!(error.0.contains("archive"), "{}", error.0);
        assert_eq!(app.archive.doc().name, "Sample archive");
    }

    #[test]
    fn a_new_archive_is_asked_for_by_its_folder() {
        let (dir, mut app) = sample(false);
        app.known_archives.clear();
        app.key(ctrl_o());
        app.picker.as_mut().unwrap().filter = "create".into();
        app.key(key(KeyCode::Enter));
        assert_eq!(
            app.input.as_ref().map(|i| i.purpose.clone()),
            Some(InputPurpose::CreateArchive)
        );
        let text = screen(&app, 120, 30);
        assert!(text.contains("The folder for the new archive"), "{text}");
        let folder = dir.path().join("New one");
        assert_eq!(
            typed(&mut app, &format!("\"{}\"", folder.display())),
            Some(Effect::CreateArchive(folder))
        );
        // Nothing typed, nothing made.
        app.key(ctrl_o());
        app.picker.as_mut().unwrap().filter = "create".into();
        app.key(key(KeyCode::Enter));
        assert_eq!(typed(&mut app, ""), None);
        assert!(
            app.message
                .as_deref()
                .unwrap()
                .starts_with("No folder given")
        );
    }

    #[test]
    fn settings_shows_the_archive_providers_and_file_types() {
        let (dir, mut app) = sample(false);
        let other = other(dir.path());
        app.known_archives = vec![other.clone()];
        app.here = Some(Here {
            providers: vec![
                "built-in".into(),
                "  extract-text Plain text, priority 0".into(),
            ],
            provider_count: (1, 0),
            formats: vec![
                "This device reads:".into(),
                "  text/plain  built-in/plain-text".into(),
            ],
            unread: 2,
            ..Here::default()
        });
        app.key(key(KeyCode::Char('8')));
        app.load_settings();
        let row = |app: &App, label: &str| {
            app.settings
                .iter()
                .position(|s| s.label == label)
                .unwrap_or_else(|| panic!("no {label} row"))
        };
        let text = screen(&app, 160, 50);
        assert!(text.contains("Providers              1 provider"), "{text}");
        assert!(
            text.contains("File types             2 types not read"),
            "{text}"
        );
        assert!(
            text.contains("Archive                Sample archive"),
            "{text}"
        );

        app.settings_chosen = row(&app, "Providers");
        let text = screen(&app, 160, 50);
        assert!(text.contains("extract-text Plain text"), "{text}");
        app.settings_chosen = row(&app, "File types");
        let text = screen(&app, 160, 50);
        assert!(text.contains("text/plain  built-in/plain-text"), "{text}");

        app.settings_chosen = row(&app, "Archive");
        let text = screen(&app, 160, 50);
        assert!(text.contains("Recent archives:"), "{text}");
        assert!(
            text.contains(&format!("Other  {}", other.display())),
            "{text}"
        );
        app.key(key(KeyCode::Enter));
        assert!(matches!(
            app.picker.as_ref().map(|p| &p.purpose),
            Some(Purpose::Archive(_))
        ));
    }
}
