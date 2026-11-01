//! The background agent on the Settings screen (#279): whether `carrel agent` is installed,
//! as what, its file, and whether an agent looks after this archive now; installing it for
//! this archive and uninstalling it, saying exactly what was written or removed, as
//! `carrel agent install` and `carrel agent uninstall` do.

use std::path::PathBuf;

use carrel_core::agent::service::{self, Manager, Places, Platform};

use super::app::{App, Confirm};
use super::review::Handled;
use super::settings::{Row, Shown};
use crate::{Failure, Report};

/// A change to the agent, asked for on the Settings screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentChange {
    /// Install it to start at log-in for these archives.
    Install(Vec<PathBuf>),
    /// Uninstall it.
    Uninstall,
}

/// Where the agent's service is read and installed.
#[derive(Debug, Clone, Default)]
pub struct Service {
    /// This platform's service manager, if Carrel knows it.
    pub platform: Option<Platform>,
    /// The folders its file goes in.
    pub places: Option<Places>,
    /// The folder agents and workers keep their files in.
    pub workers: Option<PathBuf>,
    /// Whether the service manager is run, rather than only said, as with
    /// `CARREL_SERVICE_MANAGER=off`.
    pub run_manager: bool,
}

impl Service {
    /// This device's, as `carrel agent` finds it.
    pub fn here() -> Self {
        Self {
            platform: Platform::current(),
            places: Places::here(),
            workers: crate::config::dir().ok().map(|d| d.join("workers")),
            run_manager: crate::agent::manager().1,
        }
    }

    /// The platform and folders, or why there are none.
    fn parts(&self) -> Result<(Platform, &Places), Failure> {
        let platform = self.platform.ok_or_else(|| {
            Failure::new("Carrel doesn't know how to start the agent at log-in on this platform")
        })?;
        let places = self
            .places
            .as_ref()
            .ok_or_else(|| Failure::new("there's no home folder to install the agent in"))?;
        Ok((platform, places))
    }

    /// What's installed: its file and the archives it follows, or `None`.
    pub fn installed(&self) -> Option<(PathBuf, Vec<PathBuf>)> {
        let (platform, places) = self.parts().ok()?;
        service::installed(platform, places)
    }

    /// The service manager to use, and whether it runs anything.
    fn manager(&self) -> (Box<dyn Manager>, bool) {
        if self.run_manager {
            crate::agent::manager()
        } else {
            (Box::new(Off), false)
        }
    }

    /// Makes `change`, saying exactly what it wrote, removed and ran.
    ///
    /// # Errors
    ///
    /// [`Failure`] if there's nowhere to install it, or the file can't be written or removed.
    pub fn change(&self, change: &AgentChange) -> Result<Report, Failure> {
        let (platform, places) = self.parts()?;
        let (mut manager, ran) = self.manager();
        match change {
            AgentChange::Install(roots) => {
                crate::agent::install_in(platform, places, roots, false, manager.as_mut(), ran)
            }
            AgentChange::Uninstall => {
                crate::agent::uninstall_in(platform, places, manager.as_mut(), ran)
            }
        }
    }
}

/// A service manager that runs nothing.
struct Off;

impl Manager for Off {
    fn run(&mut self, _: &str, _: &[String]) -> Result<(), String> {
        Ok(())
    }
}

impl App {
    /// This archive's root, as the agent's files name it.
    fn agent_root(&self) -> PathBuf {
        std::fs::canonicalize(self.archive.root()).unwrap_or_else(|_| self.archive.root().into())
    }

    /// The Settings screen's row for the agent.
    pub(super) fn agent_row(&self) -> Shown {
        let root = self.agent_root();
        let mut detail = Vec::new();
        let value = match (self.service.parts(), &self.service.workers) {
            (Ok((platform, places)), Some(workers)) => {
                let report = crate::agent::status_in(platform, places, workers, Some(root.clone()));
                detail.extend(report.text.lines().map(str::to_owned));
                let running = report.json["archives"]
                    .as_array()
                    .and_then(|a| {
                        a.iter()
                            .find(|a| a["path"] == root.to_string_lossy().as_ref())
                    })
                    .is_some_and(|a| a["running"] == true);
                match self.service.installed() {
                    Some((_, archives)) if archives.contains(&root) => {
                        format!(
                            "installed for this archive{}",
                            if running { ", running" } else { "" }
                        )
                    }
                    Some(_) => "installed for other archives".to_owned(),
                    None if running => "not installed; an agent runs now".to_owned(),
                    None => "not installed".to_owned(),
                }
            }
            (Err(Failure(e)), _) => {
                detail.push(format!("Not available: {e}."));
                "not available here".to_owned()
            }
            (Ok(_), None) => {
                detail.push("Not available: there's no configuration folder.".to_owned());
                "not available here".to_owned()
            }
        };
        detail.push(String::new());
        detail.push(
            "The agent drafts the queue as items arrive, keeps the index current, records work \
             sessions as they finish and takes scheduled snapshots, starting at log-in."
                .into(),
        );
        detail.push(
            "Enter installs it for this archive, keeping the archives it already looks after; \
             x uninstalls it. Both say exactly what they wrote or removed, as `carrel agent` does."
                .into(),
        );
        if !self.agent_done.is_empty() {
            detail.push(String::new());
            detail.push("What the last change did:".into());
            detail.extend(self.agent_done.iter().cloned());
        }
        Shown {
            row: Row::Agent,
            label: "Agent".into(),
            value,
            detail,
        }
    }

    /// Handles a key on the agent's row: Enter asks to install it for this archive, x to
    /// uninstall it.
    pub(super) fn agent_key(&mut self, key: ratatui::crossterm::event::KeyCode) -> Handled {
        use ratatui::crossterm::event::KeyCode;
        let installed = self.service.installed();
        match key {
            KeyCode::Enter => {
                if let Err(Failure(e)) = self.service.parts() {
                    self.message = Some(e);
                    return Handled::Yes(None);
                }
                let root = self.agent_root();
                let mut roots = installed.map(|(_, roots)| roots).unwrap_or_default();
                if !roots.contains(&root) {
                    roots.push(root);
                }
                self.confirm = Some(Confirm::Agent(AgentChange::Install(roots)));
            }
            KeyCode::Char('x') => match installed {
                Some(_) => self.confirm = Some(Confirm::Agent(AgentChange::Uninstall)),
                None => self.message = Some("The agent isn't installed; nothing to remove".into()),
            },
            _ => return Handled::No,
        }
        Handled::Yes(None)
    }

    /// Makes `change` and says what it did, in the message line and on the agent's row.
    pub(super) fn change_agent(&mut self, change: &AgentChange) -> Result<String, String> {
        let result = self.service.change(change);
        let text = match &result {
            Ok(report) => report.text.clone(),
            Err(Failure(e)) => e.clone(),
        };
        self.agent_done = text.lines().map(str::to_owned).collect();
        let line = self.agent_done.join("; ");
        result.map(|_| line.clone()).map_err(|_| line)
    }
}

/// What a confirmation of `change` asks and says.
pub fn confirm_text(change: &AgentChange) -> (String, String) {
    match change {
        AgentChange::Install(_) => (
            "Install the Carrel agent?".to_owned(),
            format!(
                "It starts at log-in and looks after {}. Carrel writes its service file in your \
                 home folder and asks the service manager to start it.",
                names(change).join(", ")
            ),
        ),
        AgentChange::Uninstall => (
            "Uninstall the Carrel agent?".to_owned(),
            "Carrel stops it and removes exactly the file it installed; the archives aren't \
             touched."
                .to_owned(),
        ),
    }
}

/// The archives `change` installs for, by name, for people.
pub fn names(change: &AgentChange) -> Vec<String> {
    match change {
        AgentChange::Install(roots) => roots
            .iter()
            .map(|root| {
                carrel_core::archive::Archive::open(root).map_or_else(
                    |_| root.display().to_string(),
                    |a| format!("“{}”", a.doc().name),
                )
            })
            .collect(),
        AgentChange::Uninstall => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::{Confirm, Effect, Tab};
    use super::super::settings::Row;
    use super::super::tests::{key, sample};
    use super::*;

    /// A service in a temporary home folder, whose manager runs nothing.
    fn temporary(dir: &std::path::Path) -> Service {
        Service {
            platform: Platform::current(),
            places: Some(Places {
                home: dir.join("home"),
                xdg_config_home: None,
                config: dir.join("config"),
            }),
            workers: Some(dir.join("config/workers")),
            run_manager: false,
        }
    }

    fn agent_row(app: &App) -> usize {
        app.settings
            .iter()
            .position(|s| s.row == Row::Agent)
            .unwrap()
    }

    #[test]
    fn settings_shows_installs_and_uninstalls_the_agent_in_a_temporary_home() {
        let (dir, mut app) = sample(false);
        if Platform::current().is_none() {
            return;
        }
        app.service = temporary(dir.path());
        app.tab = Tab::Settings;
        app.load_settings();
        let at = agent_row(&app);
        assert_eq!(app.settings[at].value, "not installed");
        assert!(
            app.settings[at].detail[0].starts_with("Not installed."),
            "{:?}",
            app.settings[at].detail
        );
        app.settings_chosen = at;

        // Nothing to uninstall yet.
        assert_eq!(app.key(key(KeyCode::Char('x'))), None);
        assert!(app.confirm.is_none());

        // Enter asks first, then installs it for this archive.
        assert_eq!(app.key(key(KeyCode::Enter)), None);
        let root = std::fs::canonicalize(app.archive.root()).unwrap();
        assert_eq!(
            app.confirm,
            Some(Confirm::Agent(AgentChange::Install(vec![root.clone()])))
        );
        let Some(Effect::Agent(change)) = app.key(key(KeyCode::Char('y'))) else {
            panic!("no effect");
        };
        let said = app.change_agent(&change).unwrap();
        let places = app.service.places.clone().unwrap();
        let file = places.file(Platform::current().unwrap());
        assert!(
            file.starts_with(dir.path().join("home"))
                || file.starts_with(dir.path().join("config"))
        );
        assert!(file.is_file());
        assert!(
            said.contains(&format!("Wrote {}", file.display())),
            "{said}"
        );
        assert!(said.contains("Didn't run"), "{said}");
        let (_, archives) = app.service.installed().unwrap();
        assert_eq!(archives, vec![root]);
        app.load_settings();
        assert_eq!(app.settings[at].value, "installed for this archive");
        assert!(
            app.settings[at]
                .detail
                .iter()
                .any(|l| l.contains(&file.display().to_string())),
            "{:?}",
            app.settings[at].detail
        );

        // x asks, then removes exactly that file.
        app.settings_chosen = at;
        assert_eq!(app.key(key(KeyCode::Char('x'))), None);
        assert_eq!(app.confirm, Some(Confirm::Agent(AgentChange::Uninstall)));
        let Some(Effect::Agent(change)) = app.key(key(KeyCode::Char('y'))) else {
            panic!("no effect");
        };
        let said = app.change_agent(&change).unwrap();
        assert!(
            said.contains(&format!("Removed {}", file.display())),
            "{said}"
        );
        assert!(!file.exists());
        app.load_settings();
        assert_eq!(app.settings[at].value, "not installed");
    }

    #[test]
    fn tests_never_touch_the_real_service() {
        let (_dir, app) = sample(false);
        assert!(app.service.places.is_none());
        assert!(!app.service.run_manager);
        let Err(Failure(e)) = app.service.change(&AgentChange::Uninstall) else {
            panic!("a test app has nowhere to install");
        };
        assert!(!e.is_empty());
    }
}
