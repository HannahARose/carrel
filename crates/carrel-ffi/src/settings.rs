//! Settings, as the app's Settings window shows them (`docs/design/mac-app.md`): the
//! archive's shared settings, each change recorded in the history as `carrel settings set`
//! records it ([`carrel_core::settings`]), and this device's preferences: its choice of
//! models, and the file picker and editor the command line and the interactive mode use.

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::providers::device::{resolve_model, second_model, text_models};
use carrel_core::settings::{Key, change};

use crate::{ArchiveHandle, CarrelError};

/// One of the archive's shared settings.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Setting {
    /// Its key, as `settings.json` writes it, such as `ingest.originals`.
    pub key: String,
    /// What it does, in words.
    pub description: String,
    /// Its value.
    pub value: String,
    /// The values it can take, when they're a fixed list; otherwise a whole number.
    pub choices: Vec<String>,
}

/// A shared setting, changed.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct SettingChanged {
    /// Its value before.
    pub from: String,
    /// Its value now.
    pub to: String,
    /// The history commit, if it changed.
    pub commit: Option<String>,
}

/// This device's choice of text models.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ModelChoices {
    /// The text model chosen to draft cards, as `provider/offer`; `None` lets Carrel choose.
    pub model: Option<String>,
    /// The model asked again when the first isn't sure where an item goes: a model, `off`,
    /// or `None` for a recommended one that's installed.
    pub second_model: Option<String>,
    /// The model that drafts now, as `provider/label`, if any is available.
    pub model_in_use: Option<String>,
    /// The model asked for a second opinion now, if any.
    pub second_in_use: Option<String>,
    /// The text models available, best first, as `provider/offer`.
    pub available: Vec<String>,
}

/// This device's preferences for the command line and the interactive mode, which
/// `carrel settings set` changes too. `None` lets Carrel choose.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct DeviceSettings {
    /// The file picker for choosing what to ingest: a known picker's name, any command (with
    /// `{out}` for the file to write paths to), or `builtin`.
    pub picker: Option<String>,
    /// The editor for cards, before `$VISUAL` and `$EDITOR`.
    pub editor: Option<String>,
}

impl From<carrel_core::settings::SettingsError> for CarrelError {
    fn from(error: carrel_core::settings::SettingsError) -> Self {
        match error {
            carrel_core::settings::SettingsError::Invalid(why) => Self::Invalid { message: why },
            error => Self::failed(error),
        }
    }
}

#[uniffi::export]
impl ArchiveHandle {
    /// The archive's shared settings, in the order they're shown.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the archive can't be read.
    pub fn settings(&self) -> Result<Vec<Setting>, CarrelError> {
        let archive = self.fresh()?;
        Ok(Key::ALL
            .iter()
            .map(|&key| Setting {
                key: key.as_str().to_owned(),
                description: key.describe().to_owned(),
                value: key.get(archive.settings()),
                choices: key
                    .choices()
                    .map(|c| c.iter().map(|&c| c.to_owned()).collect())
                    .unwrap_or_default(),
            })
            .collect())
    }

    /// Sets the shared setting `key` to `value`, as one change undone from History.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for an unknown key or a value it doesn't take,
    /// [`CarrelError::Busy`] if another writer holds the archive, or
    /// [`CarrelError::Failed`].
    pub fn set_setting(&self, key: String, value: String) -> Result<SettingChanged, CarrelError> {
        let key = Key::parse(&key).ok_or_else(|| {
            CarrelError::invalid(format!(
                "no setting {key}; settings are {}",
                Key::ALL.map(Key::as_str).join(", ")
            ))
        })?;
        let archive = self.fresh()?;
        let lease =
            DiskLock::for_archive(&archive).acquire(self.holder()?, "settings", DEFAULT_LEASE)?;
        let result = change(&archive, key, &value, &lease);
        lease.release()?;
        let changed = result?;
        Ok(SettingChanged {
            from: changed.from,
            to: changed.to,
            commit: changed.commit,
        })
    }

    /// This device's choice of text models, and what's in use. Starts the providers to find
    /// out.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the preferences can't be read.
    pub fn model_choices(&self) -> Result<ModelChoices, CarrelError> {
        let prefs = self.prefs()?;
        let registry = self.registry();
        let available = text_models(&registry);
        Ok(ModelChoices {
            model_in_use: available.first().map(|m| m.label.clone()),
            second_in_use: second_model(&registry, prefs.second_model.as_deref()).map(|m| m.label),
            available: available
                .iter()
                .map(carrel_core::providers::device::AvailableModel::name)
                .collect(),
            model: prefs.model,
            second_model: prefs.second_model,
        })
    }

    /// Chooses the text model that drafts cards on this device (`None` lets Carrel choose)
    /// and the one asked for a second opinion (`off`, or `None` for a recommended one).
    /// Names are `provider/offer`, an offer, or a provider alone.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] naming the available models if one isn't, or
    /// [`CarrelError::Failed`] if the preferences can't be saved.
    pub fn choose_models(
        &self,
        model: Option<String>,
        second_model: Option<String>,
    ) -> Result<ModelChoices, CarrelError> {
        let dir = self.config_dir()?;
        let mut prefs = self.prefs()?;
        let registry = self.registry();
        let given =
            |name: Option<String>| name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty());
        prefs.model = given(model)
            .map(|name| resolve_model(&registry, &name).map(|m| m.name()))
            .transpose()
            .map_err(CarrelError::invalid)?;
        prefs.second_model = match given(second_model) {
            Some(off) if off == "off" => Some(off),
            Some(name) => Some(
                resolve_model(&registry, &name)
                    .map_err(CarrelError::invalid)?
                    .name(),
            ),
            None => None,
        };
        prefs.save_to(&dir)?;
        self.model_choices()
    }

    /// This device's file picker and editor.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the preferences can't be read.
    pub fn device_settings(&self) -> Result<DeviceSettings, CarrelError> {
        let prefs = self.prefs()?;
        Ok(DeviceSettings {
            picker: prefs.picker,
            editor: prefs.editor,
        })
    }

    /// Sets this device's `picker` or `editor` to `value`, as `carrel settings set` does; an
    /// empty value, `default` or `None` clears it, so Carrel chooses.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] for another key, or [`CarrelError::Failed`] if the
    /// preferences can't be saved.
    pub fn set_device_setting(
        &self,
        key: String,
        value: Option<String>,
    ) -> Result<DeviceSettings, CarrelError> {
        let dir = self.config_dir()?;
        let mut prefs = self.prefs()?;
        let value = value
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty() && v != "default");
        match key.as_str() {
            "picker" => prefs.picker = value,
            "editor" => prefs.editor = value,
            _ => {
                return Err(CarrelError::invalid(format!(
                    "no device setting {key}; they're picker and editor (choose models with \
                     choose_models)"
                )));
            }
        }
        prefs.save_to(&dir)?;
        self.device_settings()
    }
}
