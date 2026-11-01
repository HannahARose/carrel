//! `carrel settings`: this device's preferences (`cli.json`) and the archive's shared settings
//! (`_System/settings.json`), which the interactive mode's Settings screen shows too
//! (`docs/design/tui-ingest.md`).

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::settings::{Key, change};
use serde_json::json;

use crate::commands::{holder, locate};
use crate::config::{Identity, Prefs};
use crate::picker::{BUILT_IN, Choice, KNOWN, choose};
use crate::{Failure, Report};

/// This device's preferences that `carrel settings set` changes.
pub const DEVICE_KEYS: [&str; 6] = [
    "picker",
    "editor",
    "model",
    "second_model",
    "librarian.model",
    "previews",
];

/// The values of the `previews` preference: `auto` shows previews where the terminal is known
/// to draw images, `off` never does, and a protocol's name uses it whatever the terminal.
pub const PREVIEW_CHOICES: [&str; 5] = ["auto", "off", "kitty", "iterm2", "sixel"];

/// How the interactive mode shows previews, for people.
#[must_use]
pub fn previews_line(setting: Option<&str>) -> String {
    match setting {
        None | Some("auto") => "auto (when the terminal is known to draw images)".into(),
        Some("off") => "off".into(),
        Some(protocol) => format!("{protocol} (chosen)"),
    }
}

/// The text model in use, and why, for people. Starts the providers to find out.
pub fn model_line(preference: Option<&str>) -> String {
    let (registry, _) = crate::providers::registry();
    let best = crate::providers::text_models(&registry).into_iter().next();
    match (preference, best) {
        (_, None) => "none available (see `carrel models`)".into(),
        (Some(chosen), Some(best)) if best.name() == chosen => {
            format!("{} (your choice)", best.label)
        }
        (Some(chosen), Some(best)) => {
            format!("{} ({chosen} is chosen but isn't available)", best.label)
        }
        (None, Some(best)) => format!("{} (Carrel's choice)", best.label),
    }
}

/// The model asked when the first isn't sure where an item goes, for people.
pub fn second_line(setting: Option<&str>) -> String {
    let (registry, _) = crate::providers::registry();
    match (setting, crate::providers::second_model(&registry, setting)) {
        (Some("off"), _) => "off (your choice)".into(),
        (Some(_), Some(m)) => format!("{} (your choice)", m.label),
        (Some(chosen), None) => format!("none ({chosen} is chosen but isn't available)"),
        (None, Some(m)) => format!("{} (recommended, installed)", m.label),
        (None, None) => "none (install a recommended model with `carrel models`)".into(),
    }
}

/// The model the librarian runs on, and why, for people.
pub fn librarian_line(setting: Option<&str>) -> String {
    let (registry, _) = crate::providers::registry();
    let chosen = carrel_core::librarian::model::choose(&registry, setting, crate::ask::memory_gb());
    match (setting, chosen) {
        (_, Ok(None)) => "none available (see `carrel models`)".into(),
        (Some(_), Ok(Some(m))) => format!("{} (your choice)", m.label),
        (None, Ok(Some(m))) => format!("{} (the strongest on this device)", m.label),
        (Some(chosen), Err(_)) => format!("none ({chosen} is chosen but isn't available)"),
        (None, Err(e)) => e,
    }
}

/// The file picker in use, and why, for people.
#[must_use]
pub fn picker_line(preference: Option<&str>) -> String {
    match (preference, choose(preference)) {
        (Some(p), Choice::BuiltIn) if p.eq_ignore_ascii_case(BUILT_IN) => {
            "the built-in browser (chosen)".into()
        }
        (Some(p), _) => format!("{p} (chosen)"),
        (None, Choice::Command(words)) => {
            format!(
                "{} (found installed)",
                words.first().map_or("", String::as_str)
            )
        }
        (None, Choice::BuiltIn) => "the built-in browser (no picker installed)".into(),
    }
}

/// `carrel settings`.
///
/// # Errors
///
/// [`Failure`] if the archive or the preferences can't be read.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let prefs = Prefs::load()?;
    let (editor, from) = crate::review::editor();
    let mut text = String::from("This device:\n");
    writeln!(text, "  picker  {}", picker_line(prefs.picker.as_deref())).unwrap();
    writeln!(text, "  editor  {editor} (from {from})").unwrap();
    let model = model_line(prefs.model.as_deref());
    writeln!(text, "  model   {model}").unwrap();
    let second = second_line(prefs.second_model.as_deref());
    writeln!(text, "  second  {second}").unwrap();
    let librarian = librarian_line(prefs.librarian_model.as_deref());
    writeln!(text, "  librarian.model  {librarian}").unwrap();
    let previews = previews_line(prefs.previews.as_deref());
    writeln!(text, "  previews {previews}").unwrap();
    let (report_log, keep_days) = crate::usage_report::preference_values(&prefs);
    writeln!(
        text,
        "  report.log {report_log}, report.keep_days {keep_days}"
    )
    .unwrap();
    let missing: Vec<String> = KNOWN
        .iter()
        .filter(|k| !k.installed())
        .filter_map(|k| k.install_here().map(|i| format!("{} ({i})", k.name)))
        .collect();
    if !missing.is_empty() {
        writeln!(text, "  other pickers: {}", missing.join(", ")).unwrap();
    }
    writeln!(
        text,
        "\nThis archive, shared by every device that opens it:"
    )
    .unwrap();
    let settings = archive.settings();
    let width = Key::ALL.iter().map(|k| k.as_str().len()).max().unwrap_or(0);
    for key in Key::ALL {
        writeln!(text, "  {:width$} {}", key.as_str(), key.get(settings)).unwrap();
    }
    text.push_str("\nChange one with `carrel settings set KEY VALUE`.\n");
    Ok(Report::new(
        text,
        json!({
            "device": {
                "picker": prefs.picker,
                "picker_in_use": picker_line(prefs.picker.as_deref()),
                "editor": editor,
                "model": prefs.model,
                "model_in_use": model,
                "second_model": prefs.second_model,
                "second_in_use": second,
                "librarian.model": prefs.librarian_model,
                "librarian_in_use": librarian,
                "previews": prefs.previews,
                "report.log": report_log,
                "report.keep_days": keep_days,
            },
            "archive": Key::ALL.iter().map(|k| (k.as_str().to_owned(), json!(k.get(settings)))).collect::<serde_json::Map<_, _>>(),
        }),
    ))
}

/// `carrel settings set KEY VALUE`: a preference for this device, or a shared setting, which
/// is recorded in history. An empty value, or `default`, clears a device preference.
///
/// # Errors
///
/// [`Failure`] if the key isn't known, the value is wrong, or saving fails.
pub fn set(archive: Option<&Path>, key: &str, value: &str) -> Result<Report, Failure> {
    if let Some(done) = crate::usage_report::set_preference(key, value) {
        return done;
    }
    if DEVICE_KEYS.contains(&key) {
        let mut prefs = Prefs::load()?;
        let cleared = value.trim().is_empty() || value.trim() == "default";
        let mut stored = (!cleared).then(|| value.trim().to_owned());
        match key {
            "picker" => prefs.picker.clone_from(&stored),
            "previews" => {
                if let Some(value) = &stored {
                    let value = value.to_lowercase();
                    if !PREVIEW_CHOICES.contains(&value.as_str()) {
                        return Err(Failure::new(format!(
                            "previews is one of {}",
                            PREVIEW_CHOICES.join(", ")
                        )));
                    }
                    stored = (value != "auto").then_some(value);
                }
                prefs.previews.clone_from(&stored);
            }
            "model" => {
                if let Some(name) = &stored {
                    let (registry, _) = crate::providers::registry();
                    stored = Some(crate::providers::resolve_model(&registry, name)?.name());
                }
                prefs.model.clone_from(&stored);
            }
            "second_model" => {
                if let Some(name) = stored.as_deref().filter(|n| *n != "off") {
                    let (registry, _) = crate::providers::registry();
                    stored = Some(crate::providers::resolve_model(&registry, name)?.name());
                }
                prefs.second_model.clone_from(&stored);
            }
            "librarian.model" => {
                if let Some(name) = &stored {
                    let (registry, _) = crate::providers::registry();
                    stored = Some(crate::providers::resolve_model(&registry, name)?.name());
                }
                prefs.librarian_model.clone_from(&stored);
            }
            _ => prefs.editor.clone_from(&stored),
        }
        prefs.save()?;
        let text = match &stored {
            None => format!("Cleared {key}; Carrel chooses for itself\n"),
            Some(stored) => format!("Set {key} to {stored} on this device\n"),
        };
        return Ok(Report::new(
            text,
            json!({ "key": key, "value": value, "device": true }),
        ));
    }
    let key = Key::parse(key).ok_or_else(|| {
        Failure::new(format!(
            "no setting {key}; settings are {}, {}, {}",
            DEVICE_KEYS.join(", "),
            crate::usage_report::PREFERENCES.join(", "),
            Key::ALL.map(Key::as_str).join(", ")
        ))
    })?;
    let archive = locate(archive)?;
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "settings", DEFAULT_LEASE)?;
    let result = change(&archive, key, value, &lease);
    lease.release()?;
    let changed = result?;
    let text = if changed.commit.is_some() {
        format!(
            "Set {} to {} (was {}); undo it from History if you change your mind\n",
            key.as_str(),
            changed.to,
            changed.from
        )
    } else {
        format!("{} is already {}\n", key.as_str(), changed.to)
    };
    Ok(Report::new(
        text,
        json!({ "key": key.as_str(), "from": changed.from, "to": changed.to, "commit": changed.commit }),
    ))
}
