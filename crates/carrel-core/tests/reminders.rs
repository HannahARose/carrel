//! Reminders (`docs/design/reminders.md`) on a copy of the sample archive, whose
//! `oscillator-sim` is checked out since 2026-10-27 with a pending entry from a 2h 10m
//! session: which are due as the `reminders` setting says, the weekly sum on the first day of
//! a week, and an agent's notifications through a fake notifier, each shown once per device.

use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::reminders::{
    Notifier, Reminder, ReminderKind, Shown, due, mark_shown, notify, shows_menu_bar,
};
use jiff::Zoned;
use serde_json::{Value, json};

const OSCILLATOR: &str = "01a18701-0500-776a-9867-080924eb019c";

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// The sample archive with `reminders` on, and `oscillator-sim` due back on `due`, if given.
fn sample(dir: &Path, reminders: &[&str], due: Option<&str>) -> Archive {
    let root = dir.join("archive");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sample-archive"),
        &root,
    );
    let path = root.join("_System/settings.json");
    let mut settings: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    settings["reminders"] = json!(reminders);
    std::fs::write(&path, settings.to_string()).unwrap();
    if let Some(due) = due {
        let card = root.join("Projects/oscillator-sim.md");
        let text = std::fs::read_to_string(&card).unwrap().replace(
            "  since: \"2026-10-27T09:12:00-07:00\"\n",
            &format!("  since: \"2026-10-27T09:12:00-07:00\"\n  due: \"{due}\"\n"),
        );
        std::fs::write(&card, text).unwrap();
    }
    Archive::open(&root).unwrap()
}

/// Noon on `day` in UTC.
fn at(day: &str) -> Zoned {
    format!("{day}T12:00:00[UTC]").parse().unwrap()
}

fn kinds(reminders: &[Reminder]) -> Vec<ReminderKind> {
    reminders.iter().map(|r| r.kind).collect()
}

#[test]
fn a_waiting_entry_and_a_due_checkout_are_reminded_as_the_setting_says() {
    let dir = tempfile::tempdir().unwrap();
    let archive = sample(dir.path(), &["idle", "due-date"], Some("2026-10-30"));
    let none = Shown::default();
    let found = due(&archive, None, &at("2026-10-29"), &none);
    assert_eq!(kinds(&found), [ReminderKind::Idle, ReminderKind::DueDate]);
    assert_eq!(
        found[0].text,
        "You worked on “oscillator-sim” for 2h 10m. Its entry is waiting for you."
    );
    assert_eq!(found[0].item.unwrap().to_string(), OSCILLATOR);
    assert_eq!(found[1].text, "“oscillator-sim” is due back tomorrow.");
    assert_eq!(
        due(&archive, None, &at("2026-10-30"), &none)[1].text,
        "“oscillator-sim” is due back today."
    );
    let overdue = due(&archive, None, &at("2026-11-02"), &none);
    assert_eq!(
        overdue[1].text,
        "“oscillator-sim” was due back on 2026-10-30, 3 days ago."
    );
    // Once a day while it's overdue: each day is its own reminder.
    assert_ne!(
        overdue[1].key,
        due(&archive, None, &at("2026-11-03"), &none)[1].key
    );
    assert_eq!(
        kinds(&due(&archive, None, &at("2026-10-28"), &none)),
        [ReminderKind::Idle],
        "two days ahead isn't due yet"
    );

    let off = sample(&dir.path().join("off"), &[], Some("2026-10-30"));
    assert_eq!(due(&off, None, &at("2026-10-30"), &none), []);
    assert!(!shows_menu_bar(&off));
    assert!(shows_menu_bar(&sample(
        &dir.path().join("bar"),
        &["menu-bar"],
        None
    )));
}

#[test]
fn the_weekly_sum_comes_on_the_first_day_carrel_runs_in_a_week() {
    let dir = tempfile::tempdir().unwrap();
    let archive = sample(dir.path(), &["weekly"], None);
    let id = archive.doc().id;
    let config = dir.path().join("config");
    // Wednesday 2026-11-18: the first run that week.
    let found = due(&archive, None, &at("2026-11-18"), &Shown::default());
    assert_eq!(kinds(&found), [ReminderKind::Weekly]);
    let text = &found[0].text;
    assert!(text.contains("1 proposal to review"), "{text}");
    assert!(text.contains("1 history entry to approve"), "{text}");
    assert!(text.contains("“oscillator-sim” out for 22 days"), "{text}");
    assert_eq!(found[0].key, "weekly:2026-W47");

    mark_shown(
        &config,
        id,
        &[found[0].key.clone()],
        at("2026-11-18").timestamp(),
    )
    .unwrap();
    let shown = Shown::load(&config);
    assert_eq!(
        due(&archive, None, &at("2026-11-18"), &shown).len(),
        1,
        "Status still shows it that day"
    );
    assert_eq!(
        due(&archive, None, &at("2026-11-20"), &shown),
        [],
        "not again that week"
    );
    assert_eq!(
        due(&archive, None, &at("2026-11-23"), &shown)[0].key,
        "weekly:2026-W48",
        "Monday starts a new week"
    );

    // Nothing waiting, nothing to sum up.
    let quiet = sample(&dir.path().join("quiet"), &["weekly"], None);
    let card = quiet.root().join("Projects/oscillator-sim.md");
    let text = std::fs::read_to_string(&card).unwrap();
    let start = text.find("checked_out:").unwrap();
    let end = text[start..].find("---").unwrap() + start;
    let text = format!("{}{}", &text[..start], &text[end..]).replace("[!pending]", "[!note]");
    std::fs::write(&card, text).unwrap();
    std::fs::remove_dir_all(quiet.root().join("_System/queue")).unwrap();
    assert_eq!(due(&quiet, None, &at("2026-11-18"), &Shown::default()), []);
}

/// A notifier that remembers what it showed, or fails while `failing`.
#[derive(Default)]
struct FakeNotifier {
    shown: Vec<(String, String)>,
    failing: bool,
}

impl Notifier for FakeNotifier {
    fn show(&mut self, archive: &str, reminder: &Reminder) -> Result<(), String> {
        if self.failing {
            return Err("no notification service".into());
        }
        self.shown.push((archive.to_owned(), reminder.text.clone()));
        Ok(())
    }
}

#[test]
fn an_agent_notifies_each_reminder_once_per_device() {
    let dir = tempfile::tempdir().unwrap();
    let archive = sample(dir.path(), &["idle", "due-date"], Some("2026-10-30"));
    let config = dir.path().join("config");
    let other_device = dir.path().join("other config");

    let mut failing = FakeNotifier {
        failing: true,
        ..FakeNotifier::default()
    };
    let notified = notify(&archive, None, &config, &at("2026-10-29"), &mut failing).unwrap();
    assert_eq!((notified.shown.len(), notified.failed.len()), (0, 2));
    assert!(
        !Shown::path(&config).exists(),
        "nothing shown, nothing recorded"
    );

    let mut notifier = FakeNotifier::default();
    let notified = notify(&archive, None, &config, &at("2026-10-29"), &mut notifier).unwrap();
    assert_eq!(notified.shown.len(), 2, "tried again");
    assert_eq!(notifier.shown[1].0, "Sample archive");
    assert_eq!(
        notifier.shown[1].1,
        "“oscillator-sim” is due back tomorrow."
    );

    let again = notify(&archive, None, &config, &at("2026-10-29"), &mut notifier).unwrap();
    assert_eq!(again.shown, [], "each is shown once");
    let next_day = notify(&archive, None, &config, &at("2026-10-30"), &mut notifier).unwrap();
    assert_eq!(
        next_day
            .shown
            .iter()
            .map(|r| r.text.as_str())
            .collect::<Vec<_>>(),
        ["“oscillator-sim” is due back today."]
    );
    // Another device keeps its own record, so it reminds its owner too.
    let mut elsewhere = FakeNotifier::default();
    notify(
        &archive,
        None,
        &other_device,
        &at("2026-10-30"),
        &mut elsewhere,
    )
    .unwrap();
    assert_eq!(elsewhere.shown.len(), 2);
    // Nothing is written in the archive.
    assert!(!archive.root().join("_System/reminders.json").exists());
}
