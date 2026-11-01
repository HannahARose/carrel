//! Reminders on the command line (`docs/design/reminders.md`): `carrel reminders` and the
//! "Reminders" lines of `carrel status` list what's due; `carrel agent` shows each as a desktop
//! notification, through `notify-send` on Linux when it's installed and a PowerShell toast on
//! Windows. Without either, the reminders wait in `carrel status`. Commands never notify.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::reminders::{Notifier, Reminder, Shown, due, notify};
use serde_json::{Value, json};

use crate::commands::locate;
use crate::config::Identity;
use crate::{Failure, Report};

/// Set to a file's path to have `carrel agent` write each notification there as a line of
/// JSON instead of showing it, as in tests.
pub const NOTIFY_LOG_ENV: &str = "CARREL_NOTIFY_LOG";

/// The reminders due in `archive` now, as this device has shown them.
pub(crate) fn due_now(archive: &Archive) -> Vec<Reminder> {
    let device = Identity::load().ok().flatten().map(|i| i.device);
    let shown = crate::config::dir()
        .map(|config| Shown::load(&config))
        .unwrap_or_default();
    due(archive, device, &jiff::Zoned::now(), &shown)
}

fn reminder_json(reminder: &Reminder) -> Value {
    json!({
        "kind": reminder.kind.as_str(),
        "key": reminder.key,
        "title": reminder.title,
        "text": reminder.text,
        "item": reminder.item.map(|i| i.to_string()),
    })
}

/// `carrel reminders`: the reminders due now.
///
/// # Errors
///
/// [`Failure`] if the archive can't be found.
pub fn list(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let due = due_now(&archive);
    let mut text = String::new();
    if due.is_empty() {
        text.push_str("No reminders are due.\n");
    }
    for reminder in &due {
        writeln!(text, "{}", reminder.text).unwrap();
    }
    Ok(Report::new(
        text,
        json!({ "reminders": due.iter().map(reminder_json).collect::<Vec<_>>() }),
    ))
}

/// The "Reminders" part of `carrel status`, and its JSON.
pub fn status_section(archive: &Archive) -> (String, Value) {
    let due = due_now(archive);
    let mut text = String::new();
    if !due.is_empty() {
        text.push_str("\nReminders:\n");
        for reminder in &due {
            writeln!(text, "  {}", reminder.text).unwrap();
        }
    }
    (
        text,
        json!(due.iter().map(reminder_json).collect::<Vec<_>>()),
    )
}

/// The systems a desktop notification is shown on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Desktop {
    /// Linux and the BSDs, through `notify-send`.
    FreeDesktop,
    /// Windows, through a PowerShell toast.
    Windows,
}

impl Desktop {
    /// This system's, if Carrel notifies on it: on a Mac the app's agent notifies instead.
    #[must_use]
    pub fn here() -> Option<Self> {
        if cfg!(windows) {
            Some(Self::Windows)
        } else if cfg!(target_os = "macos") {
            None
        } else {
            Some(Self::FreeDesktop)
        }
    }
}

/// Where the reminders from `carrel agent` go.
pub enum DesktopNotifier {
    /// A desktop notification, through `program`.
    Shown {
        /// The system.
        desktop: Desktop,
        /// `notify-send` or `powershell`.
        program: PathBuf,
    },
    /// Lines of JSON appended to a file ([`NOTIFY_LOG_ENV`]).
    Logged(PathBuf),
}

impl DesktopNotifier {
    /// The notifier here: the log `CARREL_NOTIFY_LOG` names, else this system's notification
    /// program when it's installed, else none, and the reminders wait in `carrel status`.
    #[must_use]
    pub fn here() -> Option<Self> {
        if let Some(log) = std::env::var_os(NOTIFY_LOG_ENV) {
            return Some(Self::Logged(PathBuf::from(log)));
        }
        let desktop = Desktop::here()?;
        let name = match desktop {
            Desktop::FreeDesktop => "notify-send",
            Desktop::Windows => "powershell",
        };
        let folders = carrel_core::providers::process::default_folders();
        let program = carrel_core::providers::process::find_program(name, &folders)?;
        Some(Self::Shown { desktop, program })
    }
}

impl Notifier for DesktopNotifier {
    fn show(&mut self, archive: &str, reminder: &Reminder) -> Result<(), String> {
        match self {
            Self::Logged(path) => {
                let line = json!({ "archive": archive, "reminder": reminder_json(reminder) });
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&*path)
                    .and_then(|mut f| writeln!(f, "{line}"))
                    .map_err(|e| format!("{}: {e}", path.display()))
            }
            Self::Shown { desktop, program } => {
                let args = notification_args(*desktop, &reminder.title, &reminder.text);
                let status = std::process::Command::new(&*program)
                    .args(&args)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .map_err(|e| format!("{}: {e}", program.display()))?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("{} failed ({status})", program.display()))
                }
            }
        }
    }
}

/// The application ID Windows shows PowerShell's toasts under, since Carrel's command line
/// has none of its own registered.
const POWERSHELL_APP_ID: &str =
    r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe";

/// The arguments for `notify-send` or `powershell` that show `title` and `text`.
#[must_use]
pub fn notification_args(desktop: Desktop, title: &str, text: &str) -> Vec<String> {
    match desktop {
        Desktop::FreeDesktop => vec![
            "--app-name=Carrel".into(),
            "--".into(),
            title.into(),
            text.into(),
        ],
        Desktop::Windows => {
            // Single-quoted PowerShell strings take everything literally but a quote, which
            // is doubled.
            let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
            let script = format!(
                "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] > $null; \
                 $t = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
                 $x = $t.GetElementsByTagName('text'); \
                 $x.Item(0).AppendChild($t.CreateTextNode({})) > $null; \
                 $x.Item(1).AppendChild($t.CreateTextNode({})) > $null; \
                 [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier({}).Show([Windows.UI.Notifications.ToastNotification]::new($t))",
                quote(title),
                quote(text),
                quote(POWERSHELL_APP_ID),
            );
            vec![
                "-NoProfile".into(),
                "-NonInteractive".into(),
                "-Command".into(),
                script,
            ]
        }
    }
}

/// `carrel agent`'s reminder job: shows the reminders due in `archive` that this device
/// hasn't shown, and says what it showed. Without a way to notify, it does nothing, and
/// `carrel status` lists them.
///
/// # Errors
///
/// Why, in words, if the record of what was shown can't be written.
pub fn remind_for_agent(archive: &Archive, device: Id) -> Result<Option<String>, String> {
    let Some(mut desktop) = DesktopNotifier::here() else {
        return Ok(None);
    };
    let config = crate::config::dir().map_err(|Failure(e)| e)?;
    let notified = notify(
        archive,
        Some(device),
        &config,
        &jiff::Zoned::now(),
        &mut desktop,
    )
    .map_err(|e| e.to_string())?;
    if let Some((_, why)) = notified.failed.first() {
        return Err(format!("couldn't show a reminder: {why}"));
    }
    Ok((!notified.shown.is_empty())
        .then(|| crate::commands::plural(notified.shown.len(), "reminder") + " shown"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notify_send_is_given_the_title_and_text_after_its_options() {
        assert_eq!(
            notification_args(Desktop::FreeDesktop, "Due back today", "-x is due"),
            ["--app-name=Carrel", "--", "Due back today", "-x is due"]
        );
    }

    #[test]
    fn a_powershell_toast_quotes_what_it_shows() {
        let args = notification_args(Desktop::Windows, "Due back today", "“Owner's notes” is due");
        assert_eq!(&args[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
        let script = &args[3];
        assert!(
            script.contains("CreateTextNode('Due back today')"),
            "{script}"
        );
        assert!(
            script.contains("CreateTextNode('“Owner''s notes” is due')"),
            "{script}"
        );
        assert!(
            script.contains("CreateToastNotifier('{1AC14E77"),
            "{script}"
        );
    }
}
