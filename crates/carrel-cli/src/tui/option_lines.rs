//! The commands' options the interactive mode asks for along the way, each a line to type:
//! how much `backup verify --percent` reads back, where `backup restore --to` puts an item,
//! the name `backup setup --description` gives the repository, the title and summary
//! `desk add` gives a card, where a moved working copy is for `return --from`, the words and
//! filters for `discover timeline QUERY`, the days for `discover activity --from --to`, and the
//! model for `init --embedding-model`. Leaving a line empty takes the command's default.

use std::path::PathBuf;

use carrel_core::card::Id;
use jiff::civil::Date;

use super::app::{App, Confirm, Effect, Tab};
use super::backups::{Secret, VERIFY_PERCENT};
use super::screens::{Input, InputPurpose};
use crate::Failure;

/// A file or folder to put on the desk, with the card's title and summary if the owner gave
/// them; what's left out, this device's text model drafts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskCard {
    /// The file or folder.
    pub path: PathBuf,
    /// The card's title.
    pub title: Option<String>,
    /// The card's summary.
    pub summary: Option<String>,
}

#[cfg(test)]
impl DeskCard {
    /// The file or folder alone, its card drafted.
    #[must_use]
    pub fn drafted(path: PathBuf) -> Self {
        Self {
            path,
            title: None,
            summary: None,
        }
    }
}

/// A line typed for one of the commands' options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptionLine {
    /// How much of the files' contents verifying reads back, as a percentage.
    VerifyPercent,
    /// The folder to restore this item into.
    RestoreTo {
        /// The item.
        item: String,
        /// The history commit whose snapshot to restore from.
        at: String,
    },
    /// The name for the backup repository being set up.
    BackupDescription {
        /// The repository.
        repository: PathBuf,
        /// Its password, already typed.
        password: Secret,
    },
    /// The title for a desk item's card.
    DeskTitle {
        /// The file or folder.
        path: PathBuf,
    },
    /// The summary for a desk item's card.
    DeskSummary {
        /// The file or folder.
        path: PathBuf,
        /// The title given, if any.
        title: Option<String>,
    },
    /// Where this checkout's working copy is now.
    ReturnFrom {
        /// The item.
        id: Id,
    },
    /// The timeline's words and filters.
    TimelineWords,
    /// The days activity covers.
    ActivityRange,
    /// The embedding model for the new archive in this folder.
    EmbeddingModel {
        /// The new archive's folder.
        path: PathBuf,
    },
    /// The existing archive to start history for.
    StartHistory,
}

impl OptionLine {
    /// What to type, for the help line.
    #[must_use]
    pub fn prompt(&self) -> &'static str {
        match self {
            Self::VerifyPercent => "a percentage from 1 to 100, or nothing for 1",
            Self::RestoreTo { .. } => {
                "the folder (drop it here), or nothing for beside the archive"
            }
            Self::BackupDescription { .. } => "a name, or nothing for the folder's",
            Self::DeskTitle { .. } => "the title, or nothing to draft it",
            Self::DeskSummary { .. } => "the summary, or nothing to draft it",
            Self::ReturnFrom { .. } => "where the working copy is now (drop it here)",
            Self::TimelineWords => "words and filters, or nothing for everything",
            Self::ActivityRange => "FROM TO as YYYY-MM-DD, or nothing for the last year",
            Self::EmbeddingModel { .. } => "the model's name, or nothing for the default",
            Self::StartHistory => "the archive's folder (drop it here)",
        }
    }

    /// The pop-up's title and the line above what's typed.
    #[must_use]
    pub fn text(&self) -> (&'static str, String) {
        match self {
            Self::VerifyPercent => (
                " Verify ",
                format!(
                    "How much of the files' contents to read back and check, as a percentage. \
                     More takes longer; nothing for {VERIFY_PERCENT}%:"
                ),
            ),
            Self::RestoreTo { item, .. } => (
                " Restore ",
                format!(
                    "The folder to restore {item} into (drop it here, or paste). Leave it empty \
                     for a new folder beside the archive; nothing in the archive changes:"
                ),
            ),
            Self::BackupDescription { repository, .. } => (
                " Backups ",
                format!(
                    "A name for the backups in {}, shown with each snapshot. Leave it empty to \
                     name them after the folder:",
                    repository.display()
                ),
            ),
            Self::DeskTitle { path } => (
                " Desk ",
                format!(
                    "A title for the card of {}? Leave it empty for this device's text model \
                     to draft one:",
                    path.display()
                ),
            ),
            Self::DeskSummary { .. } => (
                " Desk ",
                "A summary for its card, in a sentence or two? Leave it empty for this device's \
                 text model to draft one:"
                    .to_owned(),
            ),
            Self::ReturnFrom { .. } => (
                " Return ",
                "The working copy moved? Where is it now (drop it here, or paste)? You see what \
                 changed before anything is returned:"
                    .to_owned(),
            ),
            Self::TimelineWords => (
                " Timeline ",
                "Words to find, and filters such as collection:Finances, tag:receipts, \
                 after:2025-01 and before:2026. Leave it empty for every item:"
                    .to_owned(),
            ),
            Self::ActivityRange => (
                " Activity ",
                "The first and last day, as YYYY-MM-DD YYYY-MM-DD; only the first runs to today. \
                 Leave it empty for the last year:"
                    .to_owned(),
            ),
            Self::EmbeddingModel { .. } => (
                " Archives ",
                format!(
                    "The model every device uses to search it by meaning: {}. Leave it empty \
                     for {}:",
                    embedding_choices().join(", "),
                    carrel_core::models::DEFAULT_EMBEDDING_MODEL
                ),
            ),
            Self::StartHistory => (
                " Archives ",
                "The archive to start history for, as `carrel init` does in an archive that has \
                 none: its folder (drop it here, or paste):"
                    .to_owned(),
            ),
        }
    }
}

/// The embedding models an archive can be made with.
fn embedding_choices() -> &'static [&'static str] {
    carrel_core::settings::Key::EmbeddingModel
        .choices()
        .unwrap_or_default()
}

/// The days `text` gives: the first and last, or the first to `today`. None for nothing typed.
///
/// # Errors
///
/// [`Failure`] if a day isn't a date, or they're the wrong way round.
pub fn activity_range(text: &str, today: Date) -> Result<Option<(Date, Date)>, Failure> {
    let days: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c == ',')
        .flat_map(|w| w.split(".."))
        .filter(|w| !w.is_empty() && *w != "to")
        .collect();
    let day = |text: &str| {
        text.parse::<Date>()
            .map_err(|_| Failure::new(format!("a day is YYYY-MM-DD, not “{text}”")))
    };
    let (from, to) = match days.as_slice() {
        [] => return Ok(None),
        [from] => (day(from)?, today),
        [from, to] => (day(from)?, day(to)?),
        _ => return Err(Failure::new("give the first and last day, no more")),
    };
    if from > to {
        return Err(Failure::new("the first day must come before the last"));
    }
    Ok(Some((from, to)))
}

impl App {
    /// Opens the line to type for `line`.
    pub(super) fn ask_option(&mut self, line: OptionLine) {
        self.input = Some(Input {
            purpose: InputPurpose::Option(line),
            text: String::new(),
        });
    }

    /// What happens when a line typed for `line` is entered.
    pub(super) fn option_entered(&mut self, line: OptionLine, text: String) -> Option<Effect> {
        let given = (!text.is_empty()).then(|| text.clone());
        let path = || super::archives::typed_path(&text);
        match line {
            OptionLine::VerifyPercent => match text.parse::<u8>() {
                _ if text.is_empty() => return Some(Effect::BackupVerify(VERIFY_PERCENT)),
                Ok(percent @ 1..=100) => return Some(Effect::BackupVerify(percent)),
                _ => self.message = Some(format!("“{text}” isn't from 1 to 100; nothing checked")),
            },
            OptionLine::RestoreTo { item, at } => {
                return Some(Effect::BackupRestore {
                    item,
                    at,
                    to: path(),
                });
            }
            OptionLine::BackupDescription {
                repository,
                password,
            } => {
                return Some(Effect::BackupSetup {
                    repository,
                    password,
                    description: given,
                });
            }
            OptionLine::DeskTitle { path } => {
                self.ask_option(OptionLine::DeskSummary { path, title: given });
            }
            OptionLine::DeskSummary { path, title } => {
                self.tab = Tab::Active;
                self.confirm = Some(Confirm::DeskAdd(DeskCard {
                    path,
                    title,
                    summary: given,
                }));
            }
            OptionLine::ReturnFrom { id } => match path() {
                Some(from) => self.ask_to_return(id, Some(from)),
                None => self.message = Some("No folder given; nothing returned".into()),
            },
            OptionLine::TimelineWords => {
                self.stacks.timeline_words = text;
                self.stacks.scroll = 0;
                self.load_discover();
            }
            OptionLine::ActivityRange => {
                match activity_range(&text, crate::discover::today()) {
                    Ok(range) => self.stacks.activity_range = range,
                    Err(Failure(e)) => self.message = Some(format!("{e}; the range is as it was")),
                }
                self.stacks.scroll = 0;
                self.load_discover();
            }
            OptionLine::EmbeddingModel { path } => {
                let model = given
                    .unwrap_or_else(|| carrel_core::models::DEFAULT_EMBEDDING_MODEL.to_owned());
                if embedding_choices().contains(&model.as_str()) {
                    return Some(Effect::CreateArchive(super::archives::NewArchive {
                        path,
                        embedding_model: model,
                    }));
                }
                self.message = Some(format!(
                    "No embedding model {model}; choose one of {}. Nothing created",
                    embedding_choices().join(", ")
                ));
            }
            OptionLine::StartHistory => match path() {
                Some(path) => return Some(Effect::StartHistory(path)),
                None => self.message = Some("No folder given; nothing changed".into()),
            },
        }
        None
    }
}

/// The work for [`Effect::QuestionsAbout`]: what `carrel questions about` does, raising them
/// so Browse and Questions show them.
pub(super) fn questions_about_work(root: PathBuf, card: String) -> (String, super::Work) {
    (
        "Thinking of questions about it…".into(),
        Box::new(move || crate::questions::about(Some(&root), &card)),
    )
}

#[cfg(test)]
mod tests {
    use jiff::civil::date;
    use ratatui::crossterm::event::KeyCode;

    use super::super::active::Returning;
    use super::super::app::{App, Confirm, Effect, Tab};
    use super::super::discover::View;
    use super::super::screens::InputPurpose;
    use super::super::tests::{active_sample, choose_parts_list, key, sample, screen};
    use super::{OptionLine, activity_range};

    fn typed(app: &mut App, text: &str) -> Option<Effect> {
        for c in text.chars() {
            app.key(key(KeyCode::Char(c)));
        }
        app.key(key(KeyCode::Enter))
    }

    fn line(app: &App) -> Option<OptionLine> {
        match app.input.as_ref().map(|i| i.purpose.clone()) {
            Some(InputPurpose::Option(line)) => Some(line),
            _ => None,
        }
    }

    #[test]
    fn verify_reads_back_the_share_chosen_and_restore_goes_where_chosen() {
        let (dir, mut app) = sample(false);
        app.tab = Tab::Backups;
        app.ask_option(OptionLine::VerifyPercent);
        assert!(screen(&app, 140, 30).contains("How much of the files' contents"));
        assert_eq!(typed(&mut app, ""), Some(Effect::BackupVerify(1)));
        app.ask_option(OptionLine::VerifyPercent);
        assert_eq!(typed(&mut app, "25"), Some(Effect::BackupVerify(25)));
        app.ask_option(OptionLine::VerifyPercent);
        assert_eq!(typed(&mut app, "250"), None);
        assert!(
            app.message
                .as_deref()
                .unwrap()
                .contains("isn't from 1 to 100")
        );

        // The item, then the folder: beside the archive when it's left empty.
        let at = "abc123".to_owned();
        app.input = Some(super::super::screens::Input {
            purpose: InputPurpose::RestoreItem { at: at.clone() },
            text: String::new(),
        });
        assert_eq!(typed(&mut app, "Projects/lamp.txt"), None);
        assert!(matches!(line(&app), Some(OptionLine::RestoreTo { .. })));
        assert!(screen(&app, 140, 30).contains("The folder to restore Projects/lamp.txt into"));
        let to = dir.path().join("Restored");
        assert_eq!(
            typed(&mut app, &format!("'{}'", to.display())),
            Some(Effect::BackupRestore {
                item: "Projects/lamp.txt".into(),
                at: at.clone(),
                to: Some(to),
            })
        );
        app.ask_option(OptionLine::RestoreTo {
            item: "x".into(),
            at: at.clone(),
        });
        assert_eq!(
            typed(&mut app, ""),
            Some(Effect::BackupRestore {
                item: "x".into(),
                at,
                to: None,
            })
        );
    }

    #[test]
    fn a_moved_checkout_is_returned_from_where_it_is_and_can_trim_its_snapshots() {
        let (dir, mut app) = active_sample();
        app.tab = Tab::Active;
        let item = choose_parts_list(&mut app);
        app.key(key(KeyCode::Char('m')));
        assert!(matches!(line(&app), Some(OptionLine::ReturnFrom { .. })));
        assert!(screen(&app, 140, 30).contains("Where is it now"));

        // Moved somewhere else: what changed is worked out from there.
        let moved = dir.path().join("moved.csv");
        std::fs::copy(&item.working, &moved).unwrap();
        assert_eq!(typed(&mut app, &moved.display().to_string()), None);
        match &app.confirm {
            Some(Confirm::Return {
                from, snapshots, ..
            }) => {
                assert_eq!(from.as_deref(), Some(moved.as_path()));
                assert!(!snapshots, "no backups here");
            }
            other => panic!("{other:?}, {:?}", app.message),
        }
        assert_eq!(
            app.key(key(KeyCode::Char('t'))),
            None,
            "nothing to trim without backups"
        );

        // With backups, `t` returns it and forgets its session snapshots.
        app.confirm = Some(Confirm::Return {
            id: item.id,
            title: item.title.clone(),
            changes: String::new(),
            from: Some(moved.clone()),
            snapshots: true,
        });
        assert!(app.keys().contains(&("t", "and trim snapshots")));
        assert!(screen(&app, 140, 30).contains("forget its session snapshots"));
        assert_eq!(
            app.key(key(KeyCode::Char('t'))),
            Some(Effect::Return(Returning {
                id: item.id,
                from: Some(moved),
                trim: true,
            }))
        );
    }

    #[test]
    fn the_timeline_takes_words_and_a_date_and_activity_its_days() {
        let (_dir, mut app) = sample(false);
        app.tab = Tab::Discover;
        app.key(key(KeyCode::Char('t')));
        assert!(app.discover_keys().contains(&("/", "words")));
        assert_eq!(app.stacks.lines[0], "By when made, else filed");
        app.key(key(KeyCode::Char('b')));
        assert_eq!(app.stacks.lines[0], "By created");
        app.key(key(KeyCode::Char('/')));
        assert_eq!(line(&app), Some(OptionLine::TimelineWords));
        typed(&mut app, "lamp");
        assert_eq!(app.stacks.lines[0], "By created, for “lamp”");
        // `/` again starts from the words given.
        app.key(key(KeyCode::Char('/')));
        assert_eq!(app.input.as_ref().unwrap().text, "lamp");
        app.key(key(KeyCode::Esc));

        app.key(key(KeyCode::Char('h')));
        assert!(app.discover_keys().contains(&("r", "days")));
        app.key(key(KeyCode::Char('r')));
        assert!(screen(&app, 140, 30).contains("The first and last day"));
        typed(&mut app, "2025-01-01 2025-06-30");
        assert_eq!(app.stacks.lines[0], "From 2025-01-01 to 2025-06-30");
        app.key(key(KeyCode::Char('r')));
        typed(&mut app, "soon");
        assert!(
            app.message
                .as_deref()
                .unwrap()
                .contains("the range is as it was")
        );
        assert_eq!(app.stacks.lines[0], "From 2025-01-01 to 2025-06-30");
    }

    #[test]
    fn browse_wanders_from_an_item_and_asks_for_its_questions() {
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('5')));
        app.key(key(KeyCode::Right));
        let (card, _) = app.browse_items[app.browse_item].clone();
        assert_eq!(
            app.key(key(KeyCode::Char('A'))),
            Some(Effect::QuestionsAbout(card.clone()))
        );
        let (busy, _) = super::questions_about_work(app.archive.root().to_owned(), card.clone());
        assert_eq!(busy, "Thinking of questions about it…");

        app.key(key(KeyCode::Right));
        assert!(app.keys().contains(&("w", "wander from it")));
        app.key(key(KeyCode::Char('w')));
        assert_eq!(app.tab, Tab::Discover);
        assert_eq!(app.stacks.view, View::Wander);
        let id = app
            .archive
            .items()
            .unwrap()
            .into_iter()
            .find(|i| i.card == card)
            .unwrap()
            .id;
        assert_eq!(app.stacks.walk.as_ref().unwrap().start, Some(id));
        // The first step goes on from it, as `carrel discover wander ITEM` does.
        assert!(app.stacks.item.card.is_some());
        assert_ne!(app.stacks.item.card.as_deref(), Some(card.as_str()));
    }

    #[test]
    fn history_starts_for_an_archive_that_has_none() {
        let (dir, mut app) = sample(false);
        app.known_archives.clear();
        let root = dir.path().join("Old");
        carrel_core::archive::Archive::init(&root, &carrel_core::archive::InitOptions::new("Old"))
            .unwrap();
        app.key(ratatui::crossterm::event::KeyEvent::new(
            KeyCode::Char('o'),
            ratatui::crossterm::event::KeyModifiers::CONTROL,
        ));
        app.picker.as_mut().unwrap().filter = "start history".into();
        app.key(key(KeyCode::Enter));
        assert_eq!(line(&app), Some(OptionLine::StartHistory));
        assert_eq!(
            typed(&mut app, &root.display().to_string()),
            Some(Effect::StartHistory(root.clone()))
        );
        let message = app.start_history(&root).unwrap();
        assert!(
            message.starts_with("Started history for archive “Old”"),
            "{message}"
        );
        assert_eq!(app.archive.doc().name, "Old");
        // Once started, it says so.
        assert!(
            app.start_history(&root)
                .unwrap_err()
                .0
                .contains("has started")
        );
        // A new archive isn't made over an existing one.
        let error = app
            .create_archive(&super::super::archives::NewArchive {
                path: root,
                embedding_model: carrel_core::models::DEFAULT_EMBEDDING_MODEL.into(),
            })
            .unwrap_err();
        assert!(error.0.contains("already an archive"), "{}", error.0);
    }

    #[test]
    fn activity_takes_a_first_and_last_day_or_runs_to_today() {
        let today = date(2026, 11, 1);
        assert_eq!(activity_range("", today).unwrap(), None);
        assert_eq!(
            activity_range("2025-01-01 2025-06-30", today).unwrap(),
            Some((date(2025, 1, 1), date(2025, 6, 30)))
        );
        assert_eq!(
            activity_range("2025-01-01..2025-06-30", today).unwrap(),
            Some((date(2025, 1, 1), date(2025, 6, 30)))
        );
        assert_eq!(
            activity_range("2026-03-01", today).unwrap(),
            Some((date(2026, 3, 1), today))
        );
        assert!(activity_range("2025-06-30 2025-01-01", today).is_err());
        assert!(activity_range("June", today).is_err());
    }
}
