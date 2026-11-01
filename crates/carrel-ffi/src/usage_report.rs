//! The usage report (#330), as the app's Report view shows it: this device's decision log
//! ([`carrel_core::decisions`]) summed up for a period, with recommendations, and the log's
//! preferences. Like `carrel report`, it holds no titles, text, names or queries, unless the
//! app asks for collections by path for the owner's own eyes.

use std::collections::HashMap;

use carrel_core::card::Id;
use carrel_core::local::Prefs;

use crate::{ArchiveHandle, CarrelError};

/// The report, for people and as JSON to share.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct UsageReportText {
    /// The report in words.
    pub text: String,
    /// The same as JSON, the form `carrel report --json` gives, safe to share.
    pub json: String,
    /// Whether nothing has been recorded yet.
    pub empty: bool,
}

/// This device's decision log preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Record)]
pub struct DecisionLogSettings {
    /// Whether the log is kept (`report.log`).
    pub on: bool,
    /// How many days it keeps (`report.keep_days`).
    pub keep_days: u32,
}

/// Starts this device's decision log, unless `report.log` is off. The app calls it once as
/// it starts, as the command line does.
#[uniffi::export]
pub fn start_decision_log() {
    carrel_core::decisions::start();
}

/// This device's decision log preferences.
///
/// # Errors
///
/// [`CarrelError::Failed`] if the preferences can't be read.
#[uniffi::export]
pub fn decision_log_settings() -> Result<DecisionLogSettings, CarrelError> {
    let prefs = Prefs::load().map_err(CarrelError::failed)?;
    Ok(DecisionLogSettings {
        on: prefs.report_log != Some(false),
        keep_days: carrel_core::decisions::keep_days(&prefs),
    })
}

/// Turns the decision log on or off and sets how many days it keeps, for this device, as
/// `carrel settings set report.log` and `report.keep_days` do.
///
/// # Errors
///
/// [`CarrelError::Invalid`] for fewer than one day, or [`CarrelError::Failed`] if the
/// preferences can't be saved.
#[uniffi::export]
pub fn set_decision_log(settings: DecisionLogSettings) -> Result<(), CarrelError> {
    if settings.keep_days == 0 {
        return Err(CarrelError::Invalid {
            message: "report.keep_days is at least 1".into(),
        });
    }
    let mut prefs = Prefs::load().map_err(CarrelError::failed)?;
    prefs.report_log = (!settings.on).then_some(false);
    prefs.report_keep_days =
        (settings.keep_days != carrel_core::decisions::KEEP_DAYS).then_some(settings.keep_days);
    prefs.save().map_err(CarrelError::failed)?;
    carrel_core::decisions::start();
    Ok(())
}

#[uniffi::export]
impl ArchiveHandle {
    /// The report since `since` (a span such as `14d`, `today`, or a date), with collections
    /// by path when `names` is true, for the owner alone.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Invalid`] if `since` can't be read, or [`CarrelError::Failed`] if
    /// there's no configuration folder.
    pub fn usage_report(&self, since: String, names: bool) -> Result<UsageReportText, CarrelError> {
        let at = carrel_core::librarian::changes::parse_since(&since, &jiff::Zoned::now())
            .and_then(|s| jiff::Timestamp::from_second(s).ok())
            .ok_or_else(|| CarrelError::Invalid {
                message: format!("“{since}” isn't a span such as 14d, `today` or a date"),
            })?;
        let config = carrel_core::local::config_folder()?;
        let archive = self.fresh()?;
        let report = carrel_core::decisions::report_for(&archive, &config, at);
        let paths: Option<HashMap<Id, String>> = names.then(|| {
            archive
                .collections()
                .unwrap_or_default()
                .into_iter()
                .map(|c| (c.doc.id, c.path))
                .collect()
        });
        Ok(UsageReportText {
            text: report.text(paths.as_ref()),
            json: serde_json::to_string_pretty(&report).map_err(CarrelError::failed)?,
            empty: report.is_empty(),
        })
    }

    /// Notes that the item `id` was opened, so a search it came from records where it
    /// ranked.
    pub fn note_opened(&self, id: String) {
        if let Ok(uuid) = id.parse() {
            carrel_core::decisions::opened(&self.archive, Id(uuid));
        }
    }
}
