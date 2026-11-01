//! `carrel report`: how well Carrel is doing on this device and what to tweak, from its
//! decision log (`carrel_core::decisions`), safe to share because it holds no titles, text,
//! names or queries. The interactive mode shows it on Status.

use std::collections::HashMap;
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::decisions::report::UsageReport;

use crate::commands::locate;
use crate::{Failure, Report};

/// The period a report covers unless `--since` says.
pub const DEFAULT_SINCE: &str = "30d";

/// The moment `since` names, such as `14d`, `2w`, `today` or `2026-10-01`.
///
/// # Errors
///
/// [`Failure`] if it can't be read.
pub fn since_of(since: &str) -> Result<jiff::Timestamp, Failure> {
    carrel_core::librarian::changes::parse_since(since, &jiff::Zoned::now())
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .ok_or_else(|| {
            Failure::new(format!(
                "--since takes a span such as 14d or 2w, `today`, or a date such as 2026-10-01, not “{since}”"
            ))
        })
}

/// The report on `archive` since `since`, from this device's log.
///
/// # Errors
///
/// [`Failure`] if there's no configuration folder.
pub fn usage_report_of(archive: &Archive, since: jiff::Timestamp) -> Result<UsageReport, Failure> {
    let config = carrel_core::local::config_folder()?;
    Ok(carrel_core::decisions::report_for(archive, &config, since))
}

/// The archive's collections' paths, by identifier, for the owner's eyes only.
fn collection_names(archive: &Archive) -> HashMap<Id, String> {
    archive
        .collections()
        .unwrap_or_default()
        .into_iter()
        .map(|c| (c.doc.id, c.path))
        .collect()
}

/// What to say about the log itself: whether it's on, and how to share the report.
fn about_the_log() -> String {
    let prefs = carrel_core::local::Prefs::load().unwrap_or_default();
    if prefs.report_log == Some(false) {
        "\nThe decision log is off on this device (`carrel settings set report.log on` turns it on).\n".into()
    } else {
        format!(
            "\nNo titles, text, names or searches are in it, so it can be shared: `carrel report --json > report.json`. It keeps {} days (`report.keep_days`); `carrel settings set report.log off` stops it.\n",
            carrel_core::decisions::keep_days(&prefs)
        )
    }
}

/// `carrel report`. With `names`, collections are shown by path, for the owner alone.
///
/// # Errors
///
/// [`Failure`] if the archive can't be found or `since` can't be read.
pub fn report(archive: Option<&Path>, since: &str, names: bool) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let report = usage_report_of(&archive, since_of(since)?)?;
    let paths = names.then(|| collection_names(&archive));
    let mut text = if report.is_empty() {
        "Nothing recorded yet: the report fills in as you review, correct, answer questions and search.\n".to_owned()
    } else {
        report.text(paths.as_ref())
    };
    if !names && !report.is_empty() {
        text.push_str("\nCollections are named by identifier; `carrel report --names` shows their paths, for you alone.\n");
    }
    text.push_str(&about_the_log());
    let json = serde_json::to_value(&report).map_err(|e| Failure::new(e.to_string()))?;
    Ok(Report::new(text, json))
}

/// The Report section of the interactive mode's Status: the last 30 days, with collections
/// by path, as it's the owner's own screen. Empty in unit tests.
#[must_use]
pub fn status_lines(archive: &Archive) -> Vec<String> {
    // Unit tests never read this device's own log.
    if cfg!(test) {
        return Vec::new();
    }
    let mut lines = vec![
        String::new(),
        "Report (`carrel report` to share it):".to_owned(),
    ];
    let Ok(report) = since_of(DEFAULT_SINCE).and_then(|s| usage_report_of(archive, s)) else {
        return Vec::new();
    };
    if report.is_empty() {
        lines.push("  Nothing recorded yet.".into());
        return lines;
    }
    let names = collection_names(archive);
    lines.extend(report.text(Some(&names)).lines().map(|l| format!("  {l}")));
    lines
}

/// This device's preferences for the decision log, as `carrel settings` names them.
pub const PREFERENCES: [&str; 2] = ["report.log", "report.keep_days"];

/// The decision log's preferences in `prefs`: whether it's on, and how many days it keeps.
#[must_use]
pub fn preference_values(prefs: &carrel_core::local::Prefs) -> (String, String) {
    let on = if prefs.report_log == Some(false) {
        "off"
    } else {
        "on"
    };
    (
        on.to_owned(),
        carrel_core::decisions::keep_days(prefs).to_string(),
    )
}

/// `carrel settings set` for the decision log's preferences: `None` if `key` isn't one.
/// An empty value, or `default`, clears it.
#[must_use]
pub fn set_preference(key: &str, value: &str) -> Option<Result<Report, Failure>> {
    if !PREFERENCES.contains(&key) {
        return None;
    }
    Some((|| {
        let mut prefs = carrel_core::local::Prefs::load()?;
        let value = value.trim();
        let cleared = value.is_empty() || value == "default";
        if key == "report.log" {
            prefs.report_log = match value {
                _ if cleared => None,
                "on" => Some(true),
                "off" => Some(false),
                _ => return Err(Failure::new("report.log is on or off")),
            };
        } else {
            prefs.report_keep_days = if cleared {
                None
            } else {
                Some(value.parse().ok().filter(|d| *d >= 1).ok_or_else(|| {
                    Failure::new("report.keep_days is a whole number of days, at least 1")
                })?)
            };
        }
        prefs.save()?;
        let (on, days) = preference_values(&prefs);
        let shown = if key == "report.log" { on } else { days };
        Ok(Report::new(
            format!("Set {key} to {shown} on this device\n"),
            serde_json::json!({ "key": key, "value": shown, "device": true }),
        ))
    })())
}
