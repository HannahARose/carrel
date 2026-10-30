//! `carrel review`: proposals to file items, waiting for the owner.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::backup::Backups;
use carrel_core::card::Sensitivity;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock};
use carrel_core::ingest::filing::{Filed, Placement, file_batch};
use carrel_core::questions::redirect;
use carrel_core::review::{Confidence, ReviewEntry, ReviewQueue, State, short_id};
use serde_json::{Value, json};

use crate::commands::{holder, locate};
use crate::config::Identity;
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

/// `carrel review approve ID... | --all-high`: files the items where proposed.
pub fn approve(archive: Option<&Path>, ids: &[String], all_high: bool) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let entries = if all_high {
        let (entries, _) = ReviewQueue::of(&archive).list()?;
        entries
            .into_iter()
            .filter(|e| {
                e.state == State::Proposed
                    && e.destination.confidence == Confidence::High
                    && e.card.sensitivity < Sensitivity::Sensitive
                    && e.duplicate_of.is_none()
            })
            .collect()
    } else {
        decidable(&archive, ids)?
    };
    decide(&archive, entries, Placement::Destination)
}

/// `carrel review hold ID...`: files the items in Holding, to be placed later.
pub fn hold(archive: Option<&Path>, ids: &[String]) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let entries = decidable(&archive, ids)?;
    decide(&archive, entries, Placement::Holding)
}

/// `carrel review move-to ID COLLECTION`: proposes another destination. Nothing is filed
/// until the proposal is approved.
pub fn move_to(archive: Option<&Path>, id: &str, collection: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let queue = ReviewQueue::of(&archive);
    let mut entry = queue.find(id)?;
    if matches!(entry.state, State::Rejected | State::Archived) {
        return Err(Failure::new(format!(
            "{} is already {}",
            short_id(entry.id),
            entry.state.as_str()
        )));
    }
    let collections = archive.collections()?;
    let wanted = collection.trim_matches('/');
    let target = collections
        .iter()
        .find(|c| c.path.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| Failure::new(format!("no collection {wanted}")))?;
    let dropped = redirect(
        &archive,
        &collections,
        &mut entry,
        target,
        "chosen by the owner",
    );
    let mut text = format!(
        "{} will go to {} when approved\n",
        short_id(entry.id),
        target.path
    );
    if !dropped.is_empty() {
        writeln!(
            text,
            "Tags {} don't apply in {}, so they were left off",
            dropped.join(", "),
            target.path
        )
        .unwrap();
    }
    queue.save(&entry)?;
    Ok(Report::new(
        text,
        serde_json::to_value(&entry).unwrap_or(Value::Null),
    ))
}

/// The proposals named by `ids`, all of them found before anything changes.
fn decidable(archive: &Archive, ids: &[String]) -> Result<Vec<ReviewEntry>, Failure> {
    if ids.is_empty() {
        return Err(Failure::new("name the proposals to decide"));
    }
    let queue = ReviewQueue::of(archive);
    let mut entries: Vec<ReviewEntry> = Vec::new();
    for id in ids {
        let entry = queue.find(id)?;
        if matches!(entry.state, State::Rejected | State::Archived) {
            return Err(Failure::new(format!(
                "{} is already {}",
                short_id(entry.id),
                entry.state.as_str()
            )));
        }
        if !entries.iter().any(|e| e.id == entry.id) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

/// Files `entries` as one batch, holding the writer lease.
fn decide(
    archive: &Archive,
    entries: Vec<ReviewEntry>,
    placement: Placement,
) -> Result<Report, Failure> {
    if entries.is_empty() {
        return Ok(Report::new(
            "Nothing to file\n",
            json!({"filed": [], "failed": [], "commit": null}),
        ));
    }
    let identity = Identity::load_or_create()?;
    let mut lease =
        DiskLock::for_archive(archive).acquire(holder(&identity), "ingest", DEFAULT_LEASE)?;
    let decided = entries.into_iter().map(|e| (e, placement)).collect();
    let (backups, note) = crate::backup::for_batch(archive);
    let outcome = file_batch(
        archive,
        decided,
        &mut lease,
        identity.device,
        backups.as_ref().map(|b| b as &dyn Backups),
    );
    lease.release()?;
    let outcome = outcome?;

    let mut text = String::new();
    for filed in &outcome.filed {
        writeln!(text, "Filed {} in {}", filed.title, filed.path).unwrap();
    }
    for (entry, error) in &outcome.failed {
        writeln!(
            text,
            "Not filed {} ({}): {error}",
            short_id(entry.id),
            entry.card.title
        )
        .unwrap();
    }
    let kept: Vec<&Filed> = outcome
        .filed
        .iter()
        .filter(|f| f.kept_original.is_some())
        .collect();
    if !kept.is_empty() {
        text.push_str("\nThe originals were copied, not moved (ingest.originals is keep); delete them when you're ready:\n");
        for filed in kept {
            if let Some(path) = &filed.kept_original {
                writeln!(text, "  {}", path.display()).unwrap();
            }
        }
    }
    if let Some(line) = crate::backup::describe_outcome(&outcome, note.as_deref()) {
        writeln!(text, "{line}").unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "snapshot": outcome.snapshot.as_ref().map(|s| s.id.clone()),
            "filed": outcome.filed.iter().map(|f| json!({
                "proposal": f.entry,
                "item": f.item,
                "title": f.title,
                "path": f.path,
                "card": f.card,
                "kept_original": f.kept_original,
            })).collect::<Vec<_>>(),
            "failed": outcome.failed.iter().map(|(e, error)| json!({
                "proposal": e.id,
                "state": e.state.as_str(),
                "error": error.to_string(),
            })).collect::<Vec<_>>(),
            "commit": outcome.commit,
        }),
    ))
}
