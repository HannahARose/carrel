//! The Settings screen (`docs/design/tui-ingest.md`): this device's file picker, editor,
//! models and worker, and the archive's shared settings. It changes them the way
//! `carrel settings set` does.

use std::path::Path;

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
    /// The text model the librarian answers with.
    LibrarianModel,
    /// How Browse draws item previews.
    Previews,
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
    /// This device's decision log for `carrel report`, `report.log` or `report.keep_days`.
    Report(&'static str),
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
            | Row::LibrarianModel
            | Row::Previews
            | Row::Models
            | Row::Worker
            | Row::Agent
            | Row::Providers
            | Row::Formats
            | Row::Report(_)
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
            librarian_model_row(&prefs),
            self.previews_row(&prefs),
            models_row(),
            self.worker_row(),
            self.agent_row(),
            self.providers_row(),
            self.formats_row(),
            report_row(&prefs, "report.log"),
            report_row(&prefs, "report.keep_days"),
            self.archive_row(),
            self.index_row(),
        ];
        let settings = self.archive.settings();
        // The index's row stands in for the embedding model, which switching changes.
        for key in Key::ALL.into_iter().filter(|k| *k != Key::EmbeddingModel) {
            let mut detail = vec![
                format!("{}: {}", key.title(), key.describe()),
                String::new(),
            ];
            let takes = key.takes();
            let enter = if key.choices().is_some() && !key.takes_several() {
                "Enter to choose"
            } else {
                "Enter to change it"
            };
            detail.push(format!(
                "Takes {takes}. {enter}, or `carrel settings set {}`.",
                key.as_str()
            ));
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

    /// Finds the providers the first time Settings is shown, since it can take a moment.
    pub(super) fn settings_shown(&mut self) {
        if self.tab == super::app::Tab::Settings && self.here.is_none() {
            self.here = Some(super::embedding::Here::find());
            self.load_settings();
        }
    }

    fn previews_row(&self, prefs: &crate::config::Prefs) -> Shown {
        let now = match self.previews.protocol {
            Some(protocol) => format!("Drawing them now with {protocol:?}."),
            None => "Not drawing them now.".into(),
        };
        Shown {
            row: Row::Previews,
            label: "Previews".into(),
            value: crate::settings::previews_line(prefs.previews.as_deref()),
            detail: vec![
                "Pictures of items beside their cards in Browse.".into(),
                String::new(),
                "auto draws them in terminals known to draw images (Kitty, WezTerm, Ghostty, \
                 iTerm2, foot and others); off never does; kitty, iterm2 or sixel draws them \
                 that way whatever the terminal."
                    .into(),
                now,
                String::new(),
                format!(
                    "Enter to choose: {}. It applies at once.",
                    crate::settings::PREVIEW_CHOICES.join(", ")
                ),
            ],
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
            Row::LibrarianModel => self.model_picker("librarian.model", &["default"]),
            Row::Previews => {
                self.picker = Some(Picker {
                    collections: crate::settings::PREVIEW_CHOICES
                        .iter()
                        .map(|c| (*c).to_owned())
                        .collect(),
                    filter: String::new(),
                    chosen: 0,
                    purpose: Purpose::Setting("previews".into()),
                });
            }
            Row::Models => self.models_picker(),
            Row::Shared(setting) => match setting.choices().filter(|_| !setting.takes_several()) {
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
            Row::Report("report.log") => {
                self.picker = Some(Picker {
                    collections: vec!["on".into(), "off".into()],
                    filter: String::new(),
                    chosen: 0,
                    purpose: Purpose::Setting("report.log".into()),
                });
            }
            Row::Report(key) => {
                self.input = Some(Input {
                    purpose: InputPurpose::Setting(key.into()),
                    text: shown.value.clone(),
                });
            }
            Row::Archive => self.archive_picker(),
            Row::Index => self.index_picker(),
            Row::Worker | Row::Agent | Row::Backups | Row::Providers | Row::Formats => {}
        }
        Handled::Yes(None)
    }

    /// Opens a picker for the setting `key`: `fixed` choices, then every available model.
    fn model_picker(&mut self, key: &str, fixed: &[&str]) {
        let (registry, _) = crate::providers::registry();
        let mut choices: Vec<String> = fixed.iter().map(|c| (*c).to_owned()).collect();
        choices.extend(
            crate::providers::text_models(&registry)
                .iter()
                .map(crate::providers::TextModel::name),
        );
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

fn librarian_model_row(prefs: &crate::config::Prefs) -> Shown {
    Shown {
        row: Row::LibrarianModel,
        label: "Librarian's model".into(),
        value: prefs
            .librarian_model
            .clone()
            .unwrap_or_else(|| "the strongest on this device".into()),
        detail: vec![
            "The text model the librarian answers and looks around with (the Librarian tab, \
             and asking about items)."
                .into(),
            "Unset, Carrel uses the strongest model this device has room for.".into(),
            String::new(),
            "Enter to choose one of the available models, or default to let Carrel choose.".into(),
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
                lines.push(
                    "Enter to download one or delete one, as GGUF or MLX; Carrel asks first."
                        .into(),
                );
                lines
            },
        ),
    }
}

/// Sets the setting `key` to `value` in the archive at `root`, as `carrel settings set` does,
/// and puts it to use at once: previews are drawn the new way.
pub(super) fn set_now(app: &mut App, root: &Path, key: &str, value: &str) -> Option<super::Done> {
    let result = crate::settings::set(Some(root), key, value);
    super::finished(app, result.map(|r| r.text).map_err(|crate::Failure(e)| e));
    if key == "previews" {
        app.previews = super::preview::Previews::detected();
        app.load_settings();
    }
    None
}

/// A row for one of the decision log's preferences, `key`.
fn report_row(prefs: &crate::config::Prefs, key: &'static str) -> Shown {
    let (on, days) = crate::usage_report::preference_values(prefs);
    let (value, what) = if key == "report.log" {
        (
            on,
            "Whether this device keeps its decision log for the report: what was decided in Review, corrected, answered and found, by identifier only, never titles, text, names or searches. Enter to turn it on or off.",
        )
    } else {
        (
            days,
            "How many days the decision log keeps. Enter to change it.",
        )
    };
    Shown {
        row: Row::Report(key),
        label: key.into(),
        value,
        detail: vec![
            what.into(),
            String::new(),
            "The Report section on Status, and `carrel report`, show what it says.".into(),
        ],
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

#[cfg(test)]
mod tests {
    use ratatui::crossterm::event::KeyCode;

    use super::super::app::Effect;
    use super::super::tests::{key, sample};
    use super::Row;
    use carrel_core::settings::Key;

    #[test]
    fn every_setting_has_a_row_saying_what_it_takes() {
        let (_dir, mut app) = sample(false);
        app.key(key(KeyCode::Char('8')));
        let row = |app: &super::App, wanted: Row| {
            app.settings.iter().position(|s| s.row == wanted).unwrap()
        };
        for wanted in [Row::LibrarianModel, Row::Previews] {
            row(&app, wanted);
        }
        for setting in Key::ALL.into_iter().filter(|k| *k != Key::EmbeddingModel) {
            row(&app, Row::Shared(setting));
        }
        let confidence = &app.settings[row(&app, Row::Shared(Key::LibrarianConfidence))];
        assert!(
            confidence
                .detail
                .iter()
                .any(|l| l.contains("from 0.5 to 1")),
            "{:?}",
            confidence.detail
        );

        app.settings_chosen = row(&app, Row::Previews);
        app.key(key(KeyCode::Enter));
        let picker = app.picker.clone().expect("a choice of previews");
        assert_eq!(picker.collections, crate::settings::PREVIEW_CHOICES);
        app.key(key(KeyCode::Down));
        assert_eq!(
            app.key(key(KeyCode::Enter)),
            Some(Effect::SetSetting {
                key: "previews".into(),
                value: "off".into(),
            })
        );

        // The reminders take several at once, so they're typed rather than picked.
        app.settings_chosen = row(&app, Row::Shared(Key::Reminders));
        app.key(key(KeyCode::Enter));
        assert!(app.picker.is_none());
        assert_eq!(
            app.input.as_ref().map(|i| i.text.as_str()),
            Some("idle, weekly, menu-bar")
        );
    }
}
