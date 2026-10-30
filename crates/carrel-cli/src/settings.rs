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
pub const DEVICE_KEYS: [&str; 2] = ["picker", "editor"];

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
    for key in Key::ALL {
        writeln!(text, "  {:22} {}", key.as_str(), key.get(settings)).unwrap();
    }
    text.push_str("\nChange one with `carrel settings set KEY VALUE`.\n");
    Ok(Report::new(
        text,
        json!({
            "device": {
                "picker": prefs.picker,
                "picker_in_use": picker_line(prefs.picker.as_deref()),
                "editor": editor,
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
    if DEVICE_KEYS.contains(&key) {
        let mut prefs = Prefs::load()?;
        let cleared = value.trim().is_empty() || value.trim() == "default";
        let stored = (!cleared).then(|| value.trim().to_owned());
        if key == "picker" {
            prefs.picker = stored;
        } else {
            prefs.editor = stored;
        }
        prefs.save()?;
        let text = if cleared {
            format!("Cleared {key}; Carrel chooses for itself\n")
        } else {
            format!("Set {key} to {value} on this device\n")
        };
        return Ok(Report::new(
            text,
            json!({ "key": key, "value": value, "device": true }),
        ));
    }
    let key = Key::parse(key).ok_or_else(|| {
        Failure::new(format!(
            "no setting {key}; settings are {}, {}",
            DEVICE_KEYS.join(", "),
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
