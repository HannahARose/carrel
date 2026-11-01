//! The Settings screen (`docs/design/tui-ingest.md`): this device's file picker, editor,
//! models and worker, and the archive's shared settings. It changes them the way
//! `carrel settings set` does.

use carrel_core::settings::Key;
use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, Confirm, Effect, step};
use super::review::{Handled, Picker, Purpose};
use super::screens::{Input, InputPurpose};
use crate::picker::{BUILT_IN, KNOWN};

/// A row on the Settings screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// This device's file picker.
    Picker,
    /// This device's editor.
    Editor,
    /// The text model that drafts cards on this device.
    Model,
    /// The text model asked when the first isn't sure where an item goes.
    SecondModel,
    /// Models for local providers.
    Models,
    /// The worker that drafts queued items.
    Worker,
    /// The background agent, `carrel agent`, started at log-in.
    Agent,
    /// The providers on this device, as `carrel providers` lists them.
    Providers,
    /// The file types this device reads and those it couldn't, as `carrel formats` lists them.
    Formats,
    /// The archive shown, which the archive switcher changes.
    Archive,
    /// The embedding index search uses.
    Index,
    /// A shared setting.
    Shared(Key),
    /// Backups, shown only.
    Backups,
}

/// One row as shown: what it is, its value, and the detail beside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// The row.
    pub row: Row,
    /// Its name.
    pub label: String,
    /// Its value now.
    pub value: String,
    /// What it means and how to change it, for the detail pane.
    pub detail: Vec<String>,
}

/// Whether a row is this device's or the archive's.
#[must_use]
pub fn is_device(row: Row) -> bool {
    matches!(
        row,
        Row::Picker
            | Row::Editor
            | Row::Model
            | Row::SecondModel
            | Row::Models
            | Row::Worker
            | Row::Agent
            | Row::Providers
            | Row::Formats
    )
}

impl App {
    /// Reads every setting again, for the Settings screen.
    pub(super) fn load_settings(&mut self) {
        let prefs = crate::config::Prefs::load().unwrap_or_default();
        self.picker_preference.clone_from(&prefs.picker);
        let mut rows = vec![
            picker_row(&prefs),
            editor_row(),
            model_row(&prefs),
            second_row(&prefs),
            models_row(),
            self.worker_row(),
            self.agent_row(),
            self.providers_row(),
            self.formats_row(),
            self.archive_row(),
            self.index_row(),
        ];
        let settings = self.archive.settings();
        // The index's row stands in for the embedding model, which switching changes.
        for key in Key::ALL.into_iter().filter(|k| *k != Key::EmbeddingModel) {
            let mut detail = vec![key.describe().to_owned(), String::new()];
            match key.choices() {
                Some(choices) => {
                    detail.push(format!("One of: {}. Enter to choose.", choices.join(", ")));
                }
                None => detail.push("A whole number. Enter to change it.".into()),
            }
            detail.push("Shared by every device; each change can be undone from History.".into());
            rows.push(Shown {
                row: Row::Shared(key),
                label: key.as_str().into(),
                value: key.get(settings),
                detail,
            });
        }
        rows.push(self.backups_row());
        self.settings_chosen = self.settings_chosen.min(rows.len().saturating_sub(1));
        self.settings = rows;
    }

    fn worker_row(&self) -> Shown {
        let log = crate::worker::log_path(&self.archive)
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .unwrap_or_default();
        let mut detail = vec![
            self.worker_line()
                .unwrap_or_else(|| "Not running; it starts when you queue something.".into()),
        ];
        detail.push(String::new());
        detail.push("Its log, most recent last:".into());
        let lines: Vec<&str> = log.lines().collect();
        detail.extend(
            lines[lines.len().saturating_sub(12)..]
                .iter()
                .map(|l| (*l).to_owned()),
        );
        Shown {
            row: Row::Worker,
            label: "Worker".into(),
            value: if self.worker.running {
                "running"
            } else {
                "idle"
            }
            .into(),
            detail,
        }
    }

    fn providers_row(&self) -> Shown {
        let (value, mut detail) = match &self.here {
            None => ("finding them…".to_owned(), Vec::new()),
            Some(here) => {
                let (count, failed) = here.provider_count;
                let mut value = crate::commands::plural(count, "provider");
                if failed > 0 {
                    value = format!("{value}, {failed} couldn't start");
                }
                (value, here.providers.clone())
            }
        };
        detail.push(String::new());
        detail.push(
            "The programs that read files, draft cards and embed for search, and which is used              for each; Ctrl-R looks for them again."
                .into(),
        );
        Shown {
            row: Row::Providers,
            label: "Providers".into(),
            value,
            detail,
        }
    }

    fn formats_row(&self) -> Shown {
        let (value, mut detail) = match &self.here {
            None => ("finding them…".to_owned(), Vec::new()),
            Some(here) if here.unread == 0 => {
                ("every type so far".to_owned(), here.formats.clone())
            }
            Some(here) => (
                format!("{} not read", crate::commands::plural(here.unread, "type")),
                here.formats.clone(),
            ),
        };
        detail.push(String::new());
        detail.push(
            "Items of a type this device can't read are filed by their name and details.".into(),
        );
        Shown {
            row: Row::Formats,
            label: "File types".into(),
            value,
            detail,
        }
    }

    /// Starts finding the providers the first time Settings is shown.
    pub(super) fn settings_shown(&mut self) {
        if self.tab == super::app::Tab::Settings && self.here.is_none() {
            self.find_here();
        }
    }

    fn backups_row(&self) -> Shown {
        let backups = crate::backup::status_line(&self.archive, Some(&self.history));
        Shown {
            row: Row::Backups,
            label: "Backups".into(),
            value: backups.trim().to_owned(),
            detail: vec![
                backups.trim().to_owned(),
                String::new(),
                "Set them up with `carrel backup setup`.".into(),
            ],
        }
    }

    /// Moves the choice on the Settings screen.
    pub(super) fn settings_move(&mut self, delta: i32) {
        self.settings_chosen = step(self.settings_chosen, delta, self.settings.len());
    }

    /// Handles a key on the Settings screen.
    pub(super) fn settings_key(&mut self, key: KeyEvent) -> Handled {
        if self.settings.get(self.settings_chosen).map(|s| s.row) == Some(Row::Agent) {
            return self.agent_key(key.code);
        }
        if key.code != KeyCode::Enter {
            return Handled::No;
        }
        let Some(shown) = self.settings.get(self.settings_chosen) else {
            return Handled::Yes(None);
        };
        match shown.row {
            Row::Picker => {
                let mut choices: Vec<String> = KNOWN
                    .iter()
                    .filter(|k| k.installed())
                    .map(|k| k.name.to_owned())
                    .collect();
                choices.push(BUILT_IN.into());
                choices.extend(
                    KNOWN
                        .iter()
                        .filter(|k| !k.installed())
                        .map(|k| k.name.to_owned()),
                );
                self.picker = Some(Picker {
                    collections: choices,
                    filter: String::new(),
                    chosen: 0,
                    purpose: Purpose::Setting("picker".into()),
                });
            }
            Row::Editor => {
                self.input = Some(Input {
                    purpose: InputPurpose::Setting("editor".into()),
                    text: crate::config::Prefs::load()
                        .ok()
                        .and_then(|p| p.editor)
                        .unwrap_or_default(),
                });
            }
            Row::Model => self.model_picker("model", &["default"]),
            Row::SecondModel => self.model_picker("second_model", &["default", "off"]),
            Row::Models => {
                let available: Vec<String> = carrel_core::models::CATALOG
                    .iter()
                    .filter(|m| {
                        m.is_pinned()
                            && carrel_core::local::models_dir()
                                .is_none_or(|d| !d.join(m.file_name()).is_file())
                    })
                    .map(|m| m.name.to_owned())
                    .collect();
                if available.is_empty() {
                    self.message = Some("Every catalog model is downloaded".into());
                } else {
                    self.picker = Some(Picker {
                        collections: available,
                        filter: String::new(),
                        chosen: 0,
                        purpose: Purpose::Download,
                    });
                }
            }
            Row::Shared(setting) => match setting.choices() {
                Some(choices) => {
                    self.picker = Some(Picker {
                        collections: choices.iter().map(|c| (*c).to_owned()).collect(),
                        filter: String::new(),
                        chosen: 0,
                        purpose: Purpose::Setting(setting.as_str().into()),
                    });
                }
                None => {
                    self.input = Some(Input {
                        purpose: InputPurpose::Setting(setting.as_str().into()),
                        text: shown.value.clone(),
                    });
                }
            },
            Row::Archive => self.archive_picker(),
            Row::Index => self.index_picker(),
            Row::Worker | Row::Agent | Row::Backups | Row::Providers | Row::Formats => {}
        }
        Handled::Yes(None)
    }

    /// Opens a picker for the setting `key`: `fixed` choices, then every available model.
    fn model_picker(&mut self, key: &str, fixed: &[&str]) {
        let Some(here) = &self.here else {
            self.find_here();
            self.message = Some("Still finding the models on this device…".into());
            return;
        };
        let mut choices: Vec<String> = fixed.iter().map(|c| (*c).to_owned()).collect();
        choices.extend(here.text_models.iter().cloned());
        self.picker = Some(Picker {
            collections: choices,
            filter: String::new(),
            chosen: 0,
            purpose: Purpose::Setting(key.into()),
        });
    }

    /// Asks before downloading the catalog model `name`.
    pub(super) fn confirm_download(&mut self, name: &str) {
        if let Some(model) = carrel_core::models::find(name) {
            self.confirm = Some(Confirm::Download {
                name: model.name.to_owned(),
                about: format!(
                    "{} ({}, {} license), from huggingface.co into the models folder.",
                    model.title,
                    crate::ingest::human_size(model.size),
                    model.license
                ),
            });
        }
    }
}

fn picker_row(prefs: &crate::config::Prefs) -> Shown {
    let mut detail = vec![
        "Runs when you press i to choose what to ingest. Enter to choose another.".to_owned(),
        String::new(),
    ];
    for known in KNOWN {
        let state = if known.installed() {
            "installed".to_owned()
        } else {
            known.install_here().map_or_else(
                || "not for this system".to_owned(),
                |i| format!("install: {i}"),
            )
        };
        detail.push(format!("{:7} {}: {state}", known.name, known.about));
    }
    detail.push(String::new());
    detail.push("Carrel never installs anything itself.".into());
    Shown {
        row: Row::Picker,
        label: "File picker".into(),
        value: crate::settings::picker_line(prefs.picker.as_deref()),
        detail,
    }
}

fn editor_row() -> Shown {
    let (editor, from) = crate::review::editor();
    Shown {
        row: Row::Editor,
        label: "Editor".into(),
        value: format!("{editor} ({from})"),
        detail: vec![
            "Opens when you edit a proposal's card (e in Review).".into(),
            "Enter to set one for Carrel; leave it empty to use $VISUAL or $EDITOR.".into(),
        ],
    }
}

fn model_row(prefs: &crate::config::Prefs) -> Shown {
    Shown {
        row: Row::Model,
        label: "Model".into(),
        value: prefs
            .model
            .clone()
            .unwrap_or_else(|| "Carrel's choice".into()),
        detail: vec![
            "The text model that drafts cards, routes and rates items on this device.".into(),
            "Unset, Carrel uses the best available: Apple's on-device model where there is one, \
             then local models."
                .into(),
            String::new(),
            "Enter to choose one of the available models, or default to let Carrel choose. \
             If the chosen model goes away, Carrel uses the next best."
                .into(),
            "The benchmark's results (docs/bench/) say how each model does.".into(),
        ],
    }
}

fn second_row(prefs: &crate::config::Prefs) -> Shown {
    Shown {
        row: Row::SecondModel,
        label: "Second model".into(),
        value: prefs
            .second_model
            .clone()
            .unwrap_or_else(|| "a recommended model".into()),
        detail: vec![
            "Asked when the first model isn't sure where an item goes: what it would send to \
             Holding, or place with low confidence. The better answer is kept."
                .into(),
            String::new(),
            "By default, a model the benchmark recommends that you've installed, so a fast \
             model can file most items and a careful one looks at the rest."
                .into(),
            "Enter to choose a model, default, or off.".into(),
        ],
    }
}

fn models_row() -> Shown {
    let models = crate::models::list();
    Shown {
        row: Row::Models,
        label: "Models".into(),
        value: models.as_ref().map_or_else(
            |_| "no models folder".into(),
            |r| {
                let downloaded = r.json["catalog"].as_array().map_or(0, |c| {
                    c.iter().filter(|m| m["state"] == "downloaded").count()
                });
                format!("{downloaded} downloaded")
            },
        ),
        detail: models.map_or_else(
            |crate::Failure(e)| vec![e],
            |r| {
                let mut lines: Vec<String> = r.text.lines().map(str::to_owned).collect();
                lines.push(String::new());
                lines.push("Enter to download one; Carrel asks before downloading.".into());
                lines
            },
        ),
    }
}

/// The effect of choosing `value` for the setting `key`.
#[must_use]
pub fn set(key: &str, value: &str) -> Effect {
    Effect::SetSetting {
        key: key.to_owned(),
        value: value.to_owned(),
    }
}
