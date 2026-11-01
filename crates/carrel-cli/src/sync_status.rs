//! The parts of `carrel status` about syncing (`docs/design/sync-and-mobile.md`): whether this
//! device acts through its inbox, what of its inbox waits for the writer or was skipped, and
//! what waits in the holding folder for a snapshot.

use std::fmt::Write as _;

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::holding;
use carrel_core::inbox::{self, Sending};
use serde_json::{Value, json};

/// The inbox and holding-folder sections of `carrel status` for `device`, and what `--json`
/// shows of them as `inbox` and `holding`.
pub fn section(archive: &Archive, device: Option<Id>) -> (String, Value) {
    let (mut text, inbox) = inbox_section(archive, device);
    let (holding_text, holding) = holding_section(archive);
    text.push_str(&holding_text);
    (text, json!({"inbox": inbox, "holding": holding}))
}

fn inbox_section(archive: &Archive, device: Option<Id>) -> (String, Value) {
    let Some(device) = device else {
        return (String::new(), Value::Null);
    };
    let sending = inbox::sending(archive, device);
    let lines = inbox::status_lines(archive, device);
    let mut text = String::new();
    if sending.is_some() || !lines.is_empty() {
        text.push_str("\nInbox:\n");
    }
    match &sending {
        Some(Sending::Writer(lock)) => writeln!(
            text,
            "  Another device ({}) is the writer, so changes made here go into this device's inbox, for it to apply.",
            lock.device
        )
        .unwrap(),
        Some(unlocked @ Sending::Unlocked) => writeln!(text, "  {}", unlocked.describe()).unwrap(),
        None => {}
    }
    for line in &lines {
        writeln!(text, "  {line}").unwrap();
    }
    let json = json!({
        "through_inbox": sending.is_some(),
        "why": sending.as_ref().map(Sending::describe),
        "writer": sending.as_ref().and_then(Sending::writer),
        "waiting": inbox::waiting_from(archive, device).len(),
        "skipped": inbox::skipped(archive, device),
        "lines": lines,
    });
    (text, json)
}

fn holding_section(archive: &Archive) -> (String, Value) {
    let waiting = holding::waiting(archive);
    let mut text = String::new();
    if waiting.count > 0 {
        writeln!(
            text,
            "\nHolding folder:\n  {} version{} replaced here ({}), waiting for a snapshot{}",
            waiting.count,
            if waiting.count == 1 { "" } else { "s" },
            carrel_core::human_size(waiting.size),
            match waiting.oldest_days {
                Some(0) | None => String::new(),
                Some(1) => "; the oldest is a day old".to_owned(),
                Some(days) => format!("; the oldest is {days} days old"),
            }
        )
        .unwrap();
        for warning in &waiting.warnings {
            writeln!(text, "  warning: {warning}").unwrap();
        }
    }
    (text, serde_json::to_value(&waiting).unwrap_or(Value::Null))
}
