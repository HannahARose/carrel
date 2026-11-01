//! Sync in the interactive mode (`docs/design/sync-and-mobile.md`), doing what `carrel sync`
//! does. On Status, `s` shows the Sync view: who holds the writer lock and until when, each
//! device's inbox backlog, the holding folder, re-checks waiting, the files not downloaded
//! here, the collections kept downloaded, and the conflicted copies, where `o` and `c` settle
//! the chosen one; `m` moves the archive into iCloud Drive, copying in the background with
//! progress, then shows it in its new place. In Browse, `d` downloads the chosen item's or
//! collection's contents, and `K` sets a collection's `sync.keep_downloaded`.

use std::fmt::Write as _;
use std::path::PathBuf;

use carrel_core::archive::Archive;
use carrel_core::cloud_files::Keep;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::icloud_move::{self, MovePlan, size};
use carrel_core::sync_status::{SyncStatus, sync_status};
use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{List, ListItem, ListState, Paragraph, Wrap};

use super::app::{App, Confirm, Effect};
use super::embedding::{Progress, Shared};
use super::review::{Handled, Picker, Purpose};
use super::screens::{Input, InputPurpose};
use super::ui::Theme;
use crate::commands::plural;
use crate::{Failure, Report};

/// The Sync view's state.
#[derive(Debug, Default)]
pub struct SyncView {
    /// Whether Status shows the Sync view rather than its report.
    pub shown: bool,
    /// What it shows, read when it opens and whenever everything is read again.
    pub status: Option<SyncStatus>,
    /// The chosen conflicted copy.
    pub chosen: usize,
    /// The configuration folder a move updates: this device's, except in tests, which set
    /// their own so they never write the real one.
    pub config: Option<PathBuf>,
    /// Where the archive is being moved, so it's shown there once the move is done.
    pub moving: Option<PathBuf>,
}

/// Sync work the run loop does in the background.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncWork {
    /// Download an item's contents (by its card's path) or a collection's (by its folder), as
    /// `carrel sync download` does.
    Download(String),
    /// Set a collection's `sync.keep_downloaded`, or with `None` clear it, as
    /// `carrel sync keep` does.
    Keep {
        /// The collection's folder.
        collection: String,
        /// On, off, or as the collection above says.
        on: Option<bool>,
    },
    /// Settle a conflicted copy, as `carrel sync resolve` does.
    Resolve {
        /// The copy, relative to the archive.
        copy: String,
        /// Which to keep.
        keep: Keep,
    },
    /// Move the archive, as `carrel sync move --yes` does.
    Move {
        /// The move, checked.
        plan: MovePlan,
        /// The configuration folder whose preferences it updates.
        config: PathBuf,
    },
}

/// Sync work waiting for the owner to say yes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncConfirm {
    /// Settling a conflicted copy.
    Resolve {
        /// The copy.
        copy: String,
        /// The file it's a version of.
        original: String,
        /// Which to keep.
        keep: Keep,
    },
    /// Moving the archive.
    Move {
        /// The move, checked.
        plan: MovePlan,
        /// The configuration folder whose preferences it updates.
        config: PathBuf,
    },
}

impl SyncConfirm {
    /// The work saying yes starts.
    pub fn work(self) -> SyncWork {
        match self {
            Self::Resolve { copy, keep, .. } => SyncWork::Resolve { copy, keep },
            Self::Move { plan, config } => SyncWork::Move { plan, config },
        }
    }
}

impl App {
    /// Reads the sync status again, if the Sync view is shown.
    pub(super) fn load_sync_if_shown(&mut self) {
        if self.sync.shown {
            self.load_sync();
        }
    }

    fn load_sync(&mut self) {
        let status = sync_status(&self.archive);
        self.sync.chosen = self
            .sync
            .chosen
            .min(status.conflicts.len().saturating_sub(1));
        self.sync.status = Some(status);
    }

    /// Status's keys, with the Sync view's when it's shown: `s` opens and closes it.
    pub(super) fn status_or_sync_key(&mut self, key: KeyEvent) -> Handled {
        if !self.sync.shown {
            if key.code == KeyCode::Char('s') && !self.gaps.shown {
                self.load_sync();
                self.sync.shown = true;
                return Handled::Yes(None);
            }
            return self.status_or_gaps_key(key);
        }
        let conflicts = self.sync.status.as_ref().map_or(0, |s| s.conflicts.len());
        match key.code {
            KeyCode::Esc | KeyCode::Char('s') => self.sync.shown = false,
            KeyCode::Down | KeyCode::Char('j') => {
                self.sync.chosen = (self.sync.chosen + 1).min(conflicts.saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.sync.chosen = self.sync.chosen.saturating_sub(1);
            }
            KeyCode::Char('o') => self.confirm_resolve(Keep::Original),
            KeyCode::Char('c') => self.confirm_resolve(Keep::Copy),
            KeyCode::Char('m') => self.start_move(),
            _ => return Handled::No,
        }
        Handled::Yes(None)
    }

    /// Asks before settling the chosen conflicted copy by keeping `keep`.
    fn confirm_resolve(&mut self, keep: Keep) {
        let chosen = self
            .sync
            .status
            .as_ref()
            .and_then(|s| s.conflicts.get(self.sync.chosen));
        match chosen {
            Some(c) => {
                self.confirm = Some(Confirm::Sync(SyncConfirm::Resolve {
                    copy: c.copy.clone(),
                    original: c.original.clone(),
                    keep,
                }));
            }
            None => self.message = Some("There are no conflicted copies to settle".into()),
        }
    }

    /// Opens the line for where to move the archive, with iCloud Drive filled in.
    fn start_move(&mut self) {
        if let Some(to) = icloud_move::moved_to(self.archive.root()) {
            self.message = Some(format!(
                "This is the old copy: the archive was moved to {}; Ctrl-O opens it",
                to.display()
            ));
            return;
        }
        let suggested = icloud_move::default_destination(&self.archive);
        if suggested.is_none() {
            self.message =
                Some("iCloud Drive isn't turned on here; type the folder to move it to".into());
        }
        self.input = Some(Input {
            purpose: InputPurpose::SyncMoveTo,
            text: suggested
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        });
    }

    /// Checks the move to the folder typed, and asks before making it.
    pub(super) fn sync_move_to(&mut self, text: &str) {
        let Some(to) = super::archives::typed_path(text) else {
            self.message = Some("No folder given; nothing moved".into());
            return;
        };
        let config = self
            .sync
            .config
            .clone()
            .or_else(|| (!cfg!(test)).then(carrel_core::local::config_dir).flatten());
        let Some(config) = config else {
            self.message = Some("There's no configuration folder to note the move in".into());
            return;
        };
        match icloud_move::plan_move(&self.archive, &to) {
            Ok(plan) => self.confirm = Some(Confirm::Sync(SyncConfirm::Move { plan, config })),
            Err(e) => self.message = Some(format!("It can't move there: {e}")),
        }
    }

    /// Handles `d` and `K` in Browse: `d` downloads the chosen item's contents, or on the
    /// collections, the chosen collection's; `K` asks whether to keep a collection downloaded.
    pub(super) fn sync_browse_key(&mut self, key: KeyEvent, on_item: bool) -> Option<Handled> {
        match key.code {
            KeyCode::Char('d') if on_item => {
                let (card, _) = self.browse_items.get(self.browse_item)?;
                Some(Handled::Yes(Some(Effect::Sync(SyncWork::Download(
                    card.clone(),
                )))))
            }
            KeyCode::Char('d') => {
                let collection = self.collections.get(self.browse_collection)?;
                Some(Handled::Yes(Some(Effect::Sync(SyncWork::Download(
                    collection.path.clone(),
                )))))
            }
            KeyCode::Char('K') if !on_item => {
                let collection = self.collections.get(self.browse_collection)?;
                let path = collection.path.clone();
                let on = carrel_core::cloud_files::keeps_downloaded(&self.collections, &path);
                let own = carrel_core::cloud_files::own_setting(collection);
                self.message = Some(format!(
                    "{path}'s items are {}kept downloaded on every device ({})",
                    if on { "" } else { "not " },
                    if own.is_some() {
                        "its own setting"
                    } else {
                        "as the collection above says, or by default"
                    }
                ));
                self.picker = Some(Picker {
                    collections: vec!["on".into(), "off".into(), "default".into()],
                    filter: String::new(),
                    chosen: 0,
                    purpose: Purpose::KeepDownloaded(path),
                });
                Some(Handled::Yes(None))
            }
            _ => None,
        }
    }

    /// Browse's sync keys, for the help line: only when the archive is in a synced folder,
    /// where files can be away.
    pub(super) fn sync_browse_keys(&self) -> Vec<(&'static str, &'static str)> {
        if !DiskLock::is_synced_folder(self.archive.root()) {
            return Vec::new();
        }
        match self.browse_pane {
            super::item::Pane::Collections => vec![("d", "download"), ("K", "keep downloaded")],
            _ => vec![("d", "download")],
        }
    }

    /// The Sync view's keys, for the help line.
    pub(super) fn sync_keys(&self) -> Vec<(&'static str, &'static str)> {
        let mut keys = vec![("Esc", "back")];
        if self
            .sync
            .status
            .as_ref()
            .is_some_and(|s| !s.conflicts.is_empty())
        {
            keys.extend([("↑↓", "choose"), ("o", "keep original"), ("c", "keep copy")]);
        }
        keys.push(("m", "move into iCloud Drive"));
        keys
    }

    /// Once a move is done, shows the archive in its new place.
    pub(super) fn follow_moved_archive(&mut self) {
        let Some(to) = self.sync.moving.take() else {
            return;
        };
        if icloud_move::moved_to(self.archive.root()).as_ref() != Some(&to) {
            return;
        }
        let message = self.message.take();
        let shown = self.sync.shown;
        if let Err(Failure(e)) = self.switch_archive(&to) {
            self.message = Some(e);
            return;
        }
        self.message = message;
        if shown {
            self.sync.shown = true;
            self.load_sync();
        }
    }
}

/// The value `K`'s picker chose: its work.
pub(super) fn keep_chosen(collection: String, chosen: &str) -> Effect {
    let on = match chosen {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    };
    Effect::Sync(SyncWork::Keep { collection, on })
}

/// The work for `work` in the archive at `root`, and what the help line says meanwhile.
pub(super) fn sync_work(app: &mut App, root: PathBuf, work: SyncWork) -> (String, super::Work) {
    match work {
        SyncWork::Download(target) => (
            "Downloading it…".into(),
            Box::new(move || crate::sync::download(&Archive::open(&root)?, &target)),
        ),
        SyncWork::Keep { collection, on } => (
            "Setting sync.keep_downloaded…".into(),
            Box::new(move || {
                let setting = match on {
                    Some(true) => "on",
                    Some(false) => "off",
                    None => "default",
                };
                let result = crate::sync::keep(&Archive::open(&root)?, &collection, Some(setting));
                super::changed(&root, &result);
                result
            }),
        ),
        SyncWork::Resolve { copy, keep } => (
            "Settling the conflicted copy…".into(),
            Box::new(move || {
                let keep = if keep == Keep::Copy {
                    "copy"
                } else {
                    "original"
                };
                let result = crate::sync::resolve(&Archive::open(&root)?, &copy, keep);
                super::changed(&root, &result);
                result
            }),
        ),
        SyncWork::Move { plan, config } => {
            app.sync.moving = Some(plan.to.clone());
            move_work(
                plan,
                config,
                app.progress.clone(),
                Box::new(|| {
                    Ok(crate::commands::holder(
                        &crate::config::Identity::load_or_create()?,
                    ))
                }),
            )
        }
    }
}

/// Who takes the writer lease, found when the work starts.
type FindHolder = Box<dyn FnOnce() -> Result<Holder, Failure> + Send>;

/// Moving the archive as `plan` says, holding the writer lease taken as `holder`: copies it,
/// reporting how far it has got to `progress`, checks the copy, then notes the new place in
/// the preferences in `config` and in the old copy, which is kept.
pub(super) fn move_work(
    plan: MovePlan,
    config: PathBuf,
    progress: Shared,
    holder: FindHolder,
) -> (String, super::Work) {
    let busy = format!("Moving the archive to {}…", plan.to.display());
    let work = move || {
        let archive = Archive::open(&plan.from)?;
        let lease = DiskLock::for_archive(&archive).acquire(holder()?, "move", DEFAULT_LEASE)?;
        icloud_move::copy_archive(&plan, &lease, &mut |done, total| {
            if let Ok(mut shown) = progress.lock() {
                *shown = Some(Progress {
                    what: "Copying the archive".into(),
                    done: usize::try_from(done.saturating_mul(100) / total.max(1)).unwrap_or(100),
                    total: 100,
                    item: Some(format!("{} of {}", size(done), size(total))),
                });
            }
        })?;
        drop(lease);
        icloud_move::switch_over(&plan, &config)?;
        Ok(Report::new(
            format!(
                "Moved to {}, checked against the original; the old copy is still at {} until \
                 you remove it. Restart the agent so it uses the new place.",
                plan.to.display(),
                plan.from.display()
            ),
            serde_json::json!({"moved": true, "from": plan.from, "to": plan.to}),
        ))
    };
    (busy, Box::new(work))
}

/// The pop-up asking before sync work: its title, question, explanation and yes.
pub(super) fn confirm_text(confirm: &SyncConfirm) -> (&'static str, String, String, &'static str) {
    const UNDO: &str = "It's one change, which undo can reverse from History.";
    match confirm {
        SyncConfirm::Resolve {
            copy,
            original,
            keep: Keep::Original,
        } => (
            " Conflicted copy ",
            format!("Keep the original, {original}?"),
            format!("The conflicted copy, {copy}, goes to the Trash. {UNDO}"),
            " keep the original   ",
        ),
        SyncConfirm::Resolve { copy, original, .. } => (
            " Conflicted copy ",
            format!("Keep the copy, {copy}, in place of {original}?"),
            format!(
                "The copy takes the original's name, and the original goes to the Trash. {UNDO}"
            ),
            " keep the copy   ",
        ),
        SyncConfirm::Move { plan, .. } => (
            " Move into iCloud Drive ",
            move_question(plan),
            move_explanation(plan),
            " move it   ",
        ),
    }
}

fn move_question(plan: &MovePlan) -> String {
    format!("Move the archive to {}?", plan.to.display())
}

fn move_explanation(plan: &MovePlan) -> String {
    let mut text = format!(
        "Copies {} ({}) there{}, checks the copy against the original, and uses it from now on. \
         The old copy stays at {} until you remove it.",
        plural(plan.files, "file"),
        size(plan.bytes),
        plan.free
            .map(|f| format!(", where {} is free", size(f)))
            .unwrap_or_default(),
        plan.from.display()
    );
    if !plan.synced {
        text.push_str(" That folder isn't in iCloud Drive.");
    }
    text
}

/// The Sync view's lines, in the interactive mode's words.
pub(super) fn sync_lines(app: &App, status: &SyncStatus) -> String {
    let mut text = String::new();
    if let Some(to) = icloud_move::moved_to(app.archive.root()) {
        writeln!(
            text,
            "This is the old copy: the archive was moved to {}",
            to.display()
        )
        .unwrap();
    }
    if !status.synced {
        text.push_str("Not in a synced folder; m moves it into iCloud Drive\n");
    }
    text.push_str(&crate::commands::writer_line(
        status.writer.as_ref(),
        app.device,
    ));
    if status.inboxes.is_empty() {
        text.push_str("No inbox entries waiting\n");
    }
    for inbox in &status.inboxes {
        let whose = if app.device.is_some_and(|d| d.to_string() == inbox.device) {
            "this device".to_owned()
        } else {
            format!("device {}", inbox.device)
        };
        let waiting = crate::sync::counted(inbox.waiting, "entry", "entries");
        writeln!(text, "Inbox of {whose}: {waiting} waiting").unwrap();
    }
    holding_and_files(&mut text, status);
    if status.kept.is_empty() {
        text.push_str("No collection is kept downloaded (K on a collection in Browse)\n");
    } else {
        writeln!(text, "Kept downloaded: {}", status.kept.join(", ")).unwrap();
    }
    text
}

/// The holding folder, re-checks and files not downloaded, for [`sync_lines`].
fn holding_and_files(text: &mut String, status: &SyncStatus) {
    let held = &status.holding;
    if held.files == 0 {
        text.push_str("Holding folder: empty\n");
    } else {
        writeln!(
            text,
            "{}Holding folder: {} ({}), the oldest from {}, waiting for a snapshot{}",
            if held.warn { "⚠ " } else { "" },
            plural(held.files, "file"),
            size(held.bytes),
            held.oldest.as_deref().unwrap_or("?"),
            if held.warn {
                " — take one on a Mac soon"
            } else {
                ""
            }
        )
        .unwrap();
    }
    if status.rechecks > 0 {
        let rechecks = plural(status.rechecks, "re-check");
        writeln!(text, "{rechecks} waiting for a stronger device").unwrap();
    }
    let away = &status.not_downloaded;
    if away.files.is_empty() {
        text.push_str("Everything is downloaded on this device\n");
    } else {
        writeln!(
            text,
            "{} not downloaded here ({}); they download when needed, or d in Browse",
            plural(away.files.len(), "file"),
            size(away.bytes)
        )
        .unwrap();
    }
}

/// The Sync view: the status above, and the conflicted copies to settle below.
pub fn draw_sync(frame: &mut Frame, app: &App, theme: Theme, area: Rect) {
    let Some(status) = &app.sync.status else {
        return;
    };
    let text = sync_lines(app, status);
    if status.conflicts.is_empty() {
        let text = format!("{text}\nNo conflicted copies.");
        frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }), area);
        return;
    }
    let height = u16::try_from(text.lines().count() + 2).unwrap_or(u16::MAX);
    let [top, list_area] =
        Layout::vertical([Constraint::Length(height), Constraint::Min(0)]).areas(area);
    let heading = format!(
        "{text}\n{} for you to settle: o keeps the original, c the copy",
        crate::sync::counted(
            status.conflicts.len(),
            "conflicted copy",
            "conflicted copies"
        )
    );
    frame.render_widget(Paragraph::new(heading).wrap(Wrap { trim: false }), top);
    let items: Vec<ListItem> = status
        .conflicts
        .iter()
        .map(|c| ListItem::new(format!("{}  (a version of {})", c.copy, c.original)))
        .collect();
    let mut state = ListState::default().with_selected(Some(app.sync.chosen));
    frame.render_stateful_widget(
        List::new(items).highlight_style(theme.selected()),
        list_area,
        &mut state,
    );
}

#[cfg(test)]
mod tests {
    use carrel_core::card::Id;
    use carrel_core::cloud_files::Keep;
    use carrel_core::coordinator::Holder;
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{Confirm, Effect, Tab};
    use super::super::item::Pane;
    use super::super::screens::InputPurpose;
    use super::super::tests::{key, sample, screen};
    use super::{SyncConfirm, SyncWork};

    /// Leaves an iCloud conflicted copy of a card in the sample beside it.
    fn conflicted(app: &super::App) -> (String, String) {
        let root = app.archive.root();
        let original = "Reading/Coupled oscillators note.pdf";
        let copy = "Reading/Coupled oscillators note (Mac's conflicted copy 2026-11-01).pdf";
        std::fs::copy(root.join(original), root.join(copy)).unwrap();
        (original.to_owned(), copy.to_owned())
    }

    #[test]
    fn s_on_status_shows_sync_and_settles_a_conflicted_copy_after_asking() {
        let (_dir, mut app) = sample(false);
        app.tab = Tab::Status;
        let shown = screen(&app, 160, 40);
        assert!(shown.contains("s sync"), "{shown}");
        app.key(key(KeyCode::Char('s')));
        assert!(app.sync.shown);
        let shown = screen(&app, 160, 40);
        assert!(shown.contains("Nobody is writing"), "{shown}");
        assert!(shown.contains("1 entry waiting"), "{shown}");
        assert!(shown.contains("Holding folder: empty"), "{shown}");
        assert!(shown.contains("Everything is downloaded"), "{shown}");
        assert!(shown.contains("No conflicted copies"), "{shown}");
        assert!(shown.contains("m move into iCloud Drive"), "{shown}");

        let (original, copy) = conflicted(&app);
        app.reload().unwrap();
        let shown = screen(&app, 160, 40);
        assert!(
            shown.contains("1 conflicted copy for you to settle"),
            "{shown}"
        );
        assert!(
            shown.contains(&format!("{copy}  (a version of {original})")),
            "{shown}"
        );
        assert!(shown.contains("o keep original"), "{shown}");

        app.key(key(KeyCode::Char('c')));
        assert!(matches!(
            app.confirm,
            Some(Confirm::Sync(SyncConfirm::Resolve {
                keep: Keep::Copy,
                ..
            }))
        ));
        let shown = screen(&app, 160, 40);
        assert!(shown.contains("Keep the copy"), "{shown}");
        assert_eq!(app.key(key(KeyCode::Char('n'))), None);

        app.key(key(KeyCode::Char('o')));
        assert_eq!(
            app.key(key(KeyCode::Char('y'))),
            Some(Effect::Sync(SyncWork::Resolve {
                copy,
                keep: Keep::Original
            }))
        );
        app.key(key(KeyCode::Esc));
        assert!(!app.sync.shown);
    }

    #[test]
    fn the_sync_view_shows_inboxes_holding_and_kept_collections() {
        let (_dir, mut app) = sample(false);
        let root = app.archive.root().to_owned();
        let inbox = root.join("_System/inbox/device-a");
        std::fs::create_dir_all(&inbox).unwrap();
        std::fs::write(inbox.join("1.json"), "{}").unwrap();
        std::fs::write(inbox.join("2.json"), "{}").unwrap();
        let holding = root.join(carrel_core::sync_status::HOLDING_DIR);
        std::fs::create_dir_all(&holding).unwrap();
        std::fs::write(holding.join("old.pdf"), "x").unwrap();
        let doc = root.join("Reading/_collection.md");
        let text = std::fs::read_to_string(&doc).unwrap();
        std::fs::write(
            &doc,
            text.replacen("---\n", "---\nsync:\n  keep_downloaded: true\n", 1),
        )
        .unwrap();
        app.reload().unwrap();
        app.tab = Tab::Status;
        app.key(key(KeyCode::Char('s')));
        let shown = screen(&app, 160, 40);
        assert!(
            shown.contains("Inbox of device device-a: 2 entries waiting"),
            "{shown}"
        );
        assert!(shown.contains("Holding folder: 1 file"), "{shown}");
        assert!(shown.contains("Kept downloaded: Reading"), "{shown}");
    }

    #[test]
    fn browse_downloads_items_and_collections_and_keeps_a_collection_downloaded() {
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('5')));
        let collection = app.collections[app.browse_collection].path.clone();
        assert_eq!(
            app.key(key(KeyCode::Char('d'))),
            Some(Effect::Sync(SyncWork::Download(collection.clone())))
        );
        app.key(key(KeyCode::Char('K')));
        assert!(
            app.message
                .as_deref()
                .unwrap()
                .contains("not kept downloaded")
        );
        assert!(app.picker.is_some());
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::Sync(SyncWork::Keep {
                collection,
                on: Some(true)
            }))
        );
        app.key(key(KeyCode::Right));
        assert_eq!(app.browse_pane, Pane::Items);
        let card = app.browse_items[app.browse_item].0.clone();
        assert_eq!(
            app.key(key(KeyCode::Char('d'))),
            Some(Effect::Sync(SyncWork::Download(card)))
        );
    }

    #[test]
    fn m_moves_the_archive_after_showing_the_plan_and_copies_it_with_progress() {
        let (dir, mut app) = sample(false);
        let config = dir.path().join("config");
        app.sync.config = Some(config.clone());
        let to = dir.path().canonicalize().unwrap().join("moved");
        app.tab = Tab::Status;
        app.key(key(KeyCode::Char('s')));
        app.key(key(KeyCode::Char('m')));
        let input = app.input.as_mut().expect("a line for the folder");
        assert_eq!(input.purpose, InputPurpose::SyncMoveTo);
        input.text = to.display().to_string();
        app.key(key(KeyCode::Enter));
        let Some(Confirm::Sync(SyncConfirm::Move { plan, .. })) = &app.confirm else {
            panic!("a confirmation, not {:?} ({:?})", app.confirm, app.message);
        };
        assert_eq!(plan.to, to);
        let shown = screen(&app, 160, 40);
        assert!(shown.contains("Move the archive to"), "{shown}");
        assert!(shown.contains("old copy stays at"), "{shown}");
        assert!(shown.contains("y move it"), "{shown}");
        let Some(Effect::Sync(SyncWork::Move {
            plan,
            config: noted,
        })) = app.key(key(KeyCode::Char('y')))
        else {
            panic!("the move");
        };
        assert_eq!(noted, config);

        let holder = Holder {
            person: Id::new_v7(),
            device: Id::new_v7(),
        };
        let progress = app.progress.clone();
        let (busy, work) = super::move_work(
            plan,
            config.clone(),
            progress.clone(),
            Box::new(move || Ok(holder)),
        );
        assert!(busy.starts_with("Moving the archive"), "{busy}");
        let report = work().unwrap();
        assert!(
            report.text.contains("old copy is still at"),
            "{}",
            report.text
        );
        assert_eq!(progress.lock().unwrap().as_ref().map(|p| p.done), Some(100));
        assert!(to.join("_System").is_dir());
        assert_eq!(
            carrel_core::icloud_move::moved_to(app.archive.root()),
            Some(to.clone())
        );
        let prefs = carrel_core::local::Prefs::load_from(&config).unwrap();
        assert_eq!(prefs.archive.as_deref(), Some(to.as_path()));

        app.sync.moving = Some(to.clone());
        app.follow_moved_archive();
        assert_eq!(app.archive.root().canonicalize().unwrap(), to);
    }
}
