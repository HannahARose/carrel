//! `carrel review`: proposals to file items, waiting for the owner.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use carrel_core::review::{ReviewEntry, ReviewQueue, State, short_id};
use serde_json::{Value, json};

use crate::commands::locate;
use crate::{Failure, Report};

/// `carrel review` or `carrel review list`.
pub fn list(archive: Option<&Path>, all: bool) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let (entries, bad) = ReviewQueue::of(&archive).list()?;
    let shown: Vec<&ReviewEntry> = entries
        .iter()
        .filter(|e| all || e.state.is_open())
        .collect();

    let mut text = String::new();
    if shown.is_empty() {
        text.push_str(if all {
            "No proposals\n"
        } else {
            "Nothing to review\n"
        });
    }
    let mut by_destination: BTreeMap<&str, Vec<&ReviewEntry>> = BTreeMap::new();
    for entry in &shown {
        by_destination
            .entry(entry.destination.path.as_str())
            .or_default()
            .push(entry);
    }
    for (destination, entries) in &by_destination {
        writeln!(text, "{destination}").unwrap();
        for entry in entries {
            let mut flags = vec![format!("{:?}", entry.destination.confidence).to_lowercase()];
            if entry.state != State::Proposed {
                flags.push(entry.state.as_str().to_owned());
            }
            let sensitivity = sensitivity_name(entry);
            if sensitivity != "public" && sensitivity != "personal" {
                flags.push(sensitivity);
            }
            if entry.duplicate_of.is_some() {
                flags.push("duplicate".into());
            }
            writeln!(
                text,
                "  {}  {}  ({})",
                short_id(entry.id),
                entry.card.title,
                flags.join(", ")
            )
            .unwrap();
        }
    }
    for error in &bad {
        writeln!(text, "warning: {error}").unwrap();
    }
    let json = json!({
        "entries": shown,
        "unreadable": bad.iter().map(ToString::to_string).collect::<Vec<_>>(),
    });
    Ok(Report::new(text, json))
}

fn sensitivity_name(entry: &ReviewEntry) -> String {
    serde_json::to_value(entry.card.sensitivity)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// `carrel review show ID`.
pub fn show(archive: Option<&Path>, id: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let entry = ReviewQueue::of(&archive).find(id)?;
    let mut text = String::new();
    writeln!(text, "{}", entry.card.title).unwrap();
    let mut row = |label: &str, value: &str| writeln!(text, "  {label:<13}{value}").unwrap();
    row("proposal", &entry.id.to_string());
    row("state", entry.state.as_str());
    row("from", &entry.source.path.display().to_string());
    row(
        "to",
        &format!(
            "{} ({} confidence)",
            entry.destination.path,
            format!("{:?}", entry.destination.confidence).to_lowercase()
        ),
    );
    if let Some(rule) = &entry.destination.rule {
        row("rule", rule);
    }
    row("sensitivity", &sensitivity_name(&entry));
    if !entry.card.tags.is_empty() {
        let tags: Vec<&str> = entry.card.tags.iter().map(|t| t.0.as_str()).collect();
        row("tags", &tags.join(", "));
    }
    if !entry.proposed_tags.is_empty() {
        let tags: Vec<&str> = entry.proposed_tags.iter().map(|t| t.0.as_str()).collect();
        row(
            "new tag",
            &format!("{} (added to the collection if approved)", tags.join(", ")),
        );
    }
    if let Some(duplicate) = entry.duplicate_of {
        row("duplicate of", &duplicate.to_string());
    }
    if let Some(generated) = &entry.card.generated
        && let Some(model) = &generated.model
    {
        row("drafted by", model);
    }
    if let Some(summary) = &entry.card.summary {
        writeln!(text, "\n{}", summary.trim_end()).unwrap();
    }
    if !entry.destination.reasons.is_empty() {
        writeln!(text, "\nWhy {}:", entry.destination.path).unwrap();
        for reason in &entry.destination.reasons {
            writeln!(text, "  {reason}").unwrap();
        }
    }
    if !entry.sensitivity_reasons.is_empty() {
        writeln!(text, "\nWhy {}:", sensitivity_name(&entry)).unwrap();
        for reason in &entry.sensitivity_reasons {
            writeln!(text, "  {reason}").unwrap();
        }
    }
    if !entry.problems.is_empty() {
        writeln!(text, "\nTo settle:").unwrap();
        for problem in &entry.problems {
            writeln!(text, "  {}: {}", problem.stage, problem.message).unwrap();
        }
    }
    Ok(Report::new(
        text,
        serde_json::to_value(&entry).unwrap_or(Value::Null),
    ))
}

/// `carrel review reject ID...`: the items stay where they are.
pub fn reject(archive: Option<&Path>, ids: &[String]) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let queue = ReviewQueue::of(&archive);
    // Find them all first, so a typo changes nothing.
    let mut entries = Vec::new();
    for id in ids {
        let entry = queue.find(id)?;
        if !entry.state.is_open() {
            return Err(Failure::new(format!(
                "{} is already {}",
                short_id(entry.id),
                entry.state.as_str()
            )));
        }
        entries.push(entry);
    }
    let mut text = String::new();
    for mut entry in entries {
        entry.state = State::Rejected;
        queue.save(&entry)?;
        writeln!(
            text,
            "Rejected {}: {} stays where it is",
            short_id(entry.id),
            entry.source.path.display()
        )
        .unwrap();
    }
    Ok(Report::new(text, json!({ "rejected": ids })))
}
