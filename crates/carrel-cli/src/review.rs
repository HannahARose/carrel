//! `carrel review`: proposals to file items, waiting for the owner.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;

use carrel_core::actions::{self, Edited};
use carrel_core::archive::Archive;
use carrel_core::backup::Backups;
use carrel_core::compressed::plan::Choice;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::crawl::findings::CrawlEntry;
use carrel_core::ingest::filing::{Filed, Placement};
use carrel_core::review::{ReviewEntry, ReviewQueue, State, short_id};
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

    let crawled = carrel_core::crawl::findings::list(&archive)?;
    let mut text = String::new();
    if shown.is_empty() && crawled.is_empty() {
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
            match entry.compressed.as_ref().map(|p| p.action) {
                Some(Choice::Expand) => flags.push("expands".into()),
                Some(Choice::Keep) => flags.push("kept compressed".into()),
                Some(Choice::Ask) => flags.push("expand?".into()),
                None => {}
            }
            if carrel_core::ingest::redraft::wants_redraft(entry) {
                flags.push("draft again?".into());
            }
            if entry.duplicate_of.is_some() {
                flags.push("duplicate".into());
            } else if !entry.likely_duplicates.is_empty() {
                flags.push("likely duplicate".into());
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
    text.push_str(&crawler_section(&crawled));
    for error in &bad {
        writeln!(text, "warning: {error}").unwrap();
    }
    let json = json!({
        "entries": shown,
        "crawler": crawled,
        "unreadable": bad.iter().map(ToString::to_string).collect::<Vec<_>>(),
    });
    Ok(Report::new(text, json))
}

/// The crawler's findings, for `carrel review`.
fn crawler_section(found: &[CrawlEntry]) -> String {
    if found.is_empty() {
        return String::new();
    }
    let mut text = String::from("From the crawler (approve or reject them like proposals):\n");
    for entry in found {
        writeln!(
            text,
            "  {}  {} ({:.0}% sure): {}",
            short_id(entry.id),
            entry.finding.describe(&entry.title),
            entry.confidence * 100.0,
            entry.reason
        )
        .unwrap();
    }
    text
}

/// The crawler's findings among `ids`, and the rest, which are ingest proposals. An identifier
/// that names a proposal is a proposal, so the two never clash.
fn crawler_findings(archive: &Archive, ids: &[String]) -> (Vec<CrawlEntry>, Vec<String>) {
    let mut found = Vec::new();
    let mut rest = Vec::new();
    for id in ids {
        if ReviewQueue::of(archive).find(id).is_err()
            && let Ok(entry) = carrel_core::crawl::findings::find(archive, id)
        {
            found.push(entry);
        } else {
            rest.push(id.clone());
        }
    }
    (found, rest)
}

/// Approves the crawler's findings `found`, making each change as the owner's.
fn approve_findings(archive: &Archive, found: &[CrawlEntry]) -> Result<String, Failure> {
    let mut text = String::new();
    if found.is_empty() {
        return Ok(text);
    }
    let (registry, _) = crate::providers::registry();
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(archive).acquire(holder(&identity), "review", DEFAULT_LEASE)?;
    for entry in found {
        match carrel_core::crawl::findings::approve(archive, &registry, entry, &lease) {
            Ok(_) => writeln!(
                text,
                "Approved {}: {}",
                short_id(entry.id),
                entry.finding.describe(&entry.title)
            ),
            Err(why) => writeln!(
                text,
                "Couldn't {}: {why}",
                entry.finding.describe(&entry.title)
            ),
        }
        .unwrap();
    }
    lease.release()?;
    Ok(text)
}

fn sensitivity_name(entry: &ReviewEntry) -> String {
    serde_json::to_value(entry.card.sensitivity)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// `carrel review show ID`.
pub fn show(archive: Option<&Path>, id: &str) -> Result<Report, Failure> {
    detail(archive, id, true)
}

/// A proposal in full, with the archived items it duplicates side by side when `compare`;
/// the interactive mode draws those itself.
pub(crate) fn detail(archive: Option<&Path>, id: &str, compare: bool) -> Result<Report, Failure> {
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
    if let Some(plan) = &entry.compressed {
        row("compressed", &plan.describe(&entry.destination.path));
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
    if let Some(generated) = &entry.card.generated
        && let Some(model) = &generated.model
    {
        row("drafted by", model);
    }
    for link in &entry.card.links {
        row(
            &link.link_type.0,
            link.path.as_deref().unwrap_or(&link.id.to_string()),
        );
    }
    if let Some(summary) = &entry.card.summary {
        writeln!(text, "\n{}", summary.trim_end()).unwrap();
    }
    write_readings(&mut text, &entry);
    let comparisons = actions::compare(&archive, &entry)?;
    if compare {
        write_comparisons(&mut text, &entry, &comparisons);
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
    write_declined(&mut text, &entry);
    if !entry.problems.is_empty() {
        writeln!(text, "\nTo settle:").unwrap();
        for problem in &entry.problems {
            writeln!(text, "  {}: {}", problem.stage, problem.message).unwrap();
        }
    }
    if carrel_core::ingest::redraft::wants_redraft(&entry) {
        writeln!(
            text,
            "\nDraft it again with the models here now: `carrel review redraft {}`",
            short_id(entry.id)
        )
        .unwrap();
    }
    let mut json = serde_json::to_value(&entry).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut json {
        map.insert("comparisons".into(), json!(comparisons));
    }
    Ok(Report::new(text, json))
}

/// What else the item might be, when the drafting model wasn't sure, and how to pick one.
fn write_readings(text: &mut String, entry: &ReviewEntry) {
    use carrel_core::ingest::readings;
    if entry.readings.is_empty() {
        return;
    }
    let shown = readings::on_card(entry);
    writeln!(
        text,
        "\n{} (pick one with `carrel review pick {} N`):",
        if readings::doubted(&entry.readings) {
            "What it is isn't clear. Readings"
        } else {
            "Readings"
        },
        short_id(entry.id)
    )
    .unwrap();
    for (i, reading) in entry.readings.iter().enumerate() {
        let current = if shown == Some(i) { "  (the card)" } else { "" };
        writeln!(text, "  {}. {}{current}", i + 1, reading.title).unwrap();
        writeln!(text, "     {}", reading.summary).unwrap();
        if !reading.tags.is_empty() {
            let tags: Vec<&str> = reading.tags.iter().map(|t| t.0.as_str()).collect();
            writeln!(text, "     tags: {}", tags.join(", ")).unwrap();
        }
        if !reading.reason.is_empty() {
            writeln!(text, "     why: {}", reading.reason).unwrap();
        }
        if !reading.against.is_empty() {
            writeln!(text, "     doesn't fit: {}", reading.against.join("; ")).unwrap();
        }
    }
}

/// `carrel review pick ID N`: rewrites the proposal's title, summary and tags from its reading
/// `n`, as one correction, shown before it's saved.
///
/// # Errors
///
/// [`Failure`] if the proposal or the reading isn't found, or it can't be saved.
pub fn pick(archive: Option<&Path>, id: &str, n: usize, yes: bool) -> Result<Report, Failure> {
    let words = carrel_core::ingest::readings::pick_words(n);
    crate::correct::review(archive, id, &[words], yes, false)
}

/// The models that declined the proposal, a safety refusal, and whether another stepped in.
fn write_declined(text: &mut String, entry: &ReviewEntry) {
    let declined: Vec<_> = entry
        .attempts
        .iter()
        .filter(|a| a.result == "declined")
        .collect();
    if !declined.is_empty() {
        writeln!(text, "\nDeclined (a model's safety refusal):").unwrap();
        for attempt in declined {
            writeln!(
                text,
                "  {}: {}/{} declined it{}",
                attempt.stage,
                attempt.provider,
                attempt.offer,
                if entry
                    .attempts
                    .iter()
                    .any(|a| a.stage == attempt.stage && a.result == "ok")
                {
                    "; another model did it instead"
                } else {
                    ""
                }
            )
            .unwrap();
        }
    }
}

/// Each archived item `entry` duplicates beside it, and how to decide.
fn write_comparisons(text: &mut String, entry: &ReviewEntry, comparisons: &[actions::Comparison]) {
    for comparison in comparisons {
        write_comparison(text, comparison);
    }
    if !comparisons.is_empty() && entry.state.is_open() {
        let id = short_id(entry.id);
        writeln!(
            text,
            "\n  File as a new version:  carrel review approve {id} --as-version-of ITEM\n  \
             Keep both:              carrel review approve {id} --related-to ITEM\n  \
             Skip it:                carrel review reject {id}"
        )
        .unwrap();
    }
}

/// The proposal beside an archived item, as a table; rows that differ are marked with `*`.
fn write_comparison(text: &mut String, comparison: &actions::Comparison) {
    writeln!(
        text,
        "\n{} {} ({})",
        if comparison.why == ["same checksum"] {
            "Duplicate of"
        } else {
            "Likely duplicate of"
        },
        comparison.card,
        comparison.why.join(", ")
    )
    .unwrap();
    writeln!(text, "  item {}", comparison.id).unwrap();
    let width = comparison
        .rows
        .iter()
        .map(|r| r.proposed.chars().count())
        .max()
        .unwrap_or(0)
        .clamp(8, 40);
    writeln!(
        text,
        "    {}{:<width$}  archived",
        " ".repeat(12),
        "this item"
    )
    .unwrap();
    for row in &comparison.rows {
        let proposed: String = if row.proposed.chars().count() > width {
            let cut: String = row.proposed.chars().take(width - 1).collect();
            format!("{cut}…")
        } else {
            row.proposed.clone()
        };
        writeln!(
            text,
            "  {} {:<12}{proposed:<width$}  {}",
            if row.differs { '*' } else { ' ' },
            row.label,
            row.archived
        )
        .unwrap();
    }
}

/// `carrel review reject ID...`: the items stay where they are.
pub fn reject(archive: Option<&Path>, ids: &[String]) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let (found, ids) = crawler_findings(&archive, ids);
    let mut text = String::new();
    if !found.is_empty() {
        let identity = Identity::load_or_create()?;
        for entry in &found {
            carrel_core::crawl::findings::reject(&archive, entry, identity.device)?;
            writeln!(
                text,
                "Rejected {}: the crawler won't suggest it again",
                short_id(entry.id)
            )
            .unwrap();
        }
        if ids.is_empty() {
            let rejected: Vec<String> = found.iter().map(|e| e.id.to_string()).collect();
            return Ok(Report::new(text, json!({ "rejected": rejected })));
        }
    }
    let ids = ids.as_slice();
    let entries = actions::proposals(&archive, ids)?;
    // Approved and held proposals are on their way; rejecting is for those still undecided.
    if let Some(entry) = entries.iter().find(|e| !e.state.is_open()) {
        return Err(Failure::new(format!(
            "{} is already {}",
            short_id(entry.id),
            entry.state.as_str()
        )));
    }
    for entry in actions::reject(&archive, entries)? {
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

/// `carrel review approve ID... | --all-high`: files the items where proposed, linked to an
/// archived item with `--as-version-of` or `--related-to` (an empty target when none was
/// given).
pub fn approve(
    archive: Option<&Path>,
    ids: &[String],
    all_high: bool,
    as_version_of: Option<&str>,
    related_to: Option<&str>,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let (found, ids) = crawler_findings(&archive, ids);
    let approved = approve_findings(&archive, &found)?;
    if !all_high && ids.is_empty() && !found.is_empty() {
        let ids: Vec<String> = found.iter().map(|e| e.id.to_string()).collect();
        return Ok(Report::new(approved, json!({ "approved": ids })));
    }
    let mut entries = if all_high {
        actions::all_high(&archive)?
    } else {
        actions::proposals(&archive, &ids)?
    };
    let kept = as_version_of
        .map(|t| (actions::Kept::AsVersion, t))
        .or(related_to.map(|t| (actions::Kept::Both, t)));
    if let Some((kept, target)) = kept {
        let target = Some(target).filter(|t| !t.is_empty());
        for entry in &mut entries {
            actions::keep(&archive, entry, kept, target)?;
        }
    }
    let mut report = decide(&archive, entries, Placement::Destination)?;
    report.text.insert_str(0, &approved);
    Ok(report)
}

/// `carrel review hold ID...`: files the items in Holding, to be placed later.
pub fn hold(archive: Option<&Path>, ids: &[String]) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let entries = actions::proposals(&archive, ids)?;
    decide(&archive, entries, Placement::Holding)
}

/// `carrel review move-to ID COLLECTION`: proposes another destination. Nothing is filed
/// until the proposal is approved.
pub fn move_to(archive: Option<&Path>, id: &str, collection: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let moved = actions::move_to(&archive, id, collection)?;
    let mut text = format!(
        "{} will go to {} when approved\n",
        short_id(moved.entry.id),
        moved.to
    );
    if !moved.dropped.is_empty() {
        writeln!(
            text,
            "Tags {} don't apply in {}, so they were left off",
            moved.dropped.join(", "),
            moved.to
        )
        .unwrap();
    }
    Ok(Report::new(
        text,
        serde_json::to_value(&moved.entry).unwrap_or(Value::Null),
    ))
}

/// `carrel review expand ID...` and `carrel review keep-compressed ID...`: say whether
/// compressed files are expanded when approved or kept as they are.
pub fn choose_compressed(
    archive: Option<&Path>,
    ids: &[String],
    choice: Choice,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let entries = actions::proposals(&archive, ids)?;
    let mut text = String::new();
    let mut saved = Vec::new();
    for entry in entries {
        let entry = actions::choose_compressed(&archive, &entry.id.to_string(), choice)?;
        let plan = entry.compressed.as_ref().expect("a compressed file");
        writeln!(
            text,
            "{} {}: {} when approved",
            short_id(entry.id),
            entry.card.title,
            plan.describe(&entry.destination.path)
        )
        .unwrap();
        saved.push(entry);
    }
    Ok(Report::new(text, json!({ "entries": saved })))
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
    let (backups, note) = crate::backup::for_batch(archive);
    let outcome = actions::decide(
        archive,
        entries,
        placement,
        holder(&identity),
        backups.as_ref().map(|b| b as &dyn Backups),
    )?;

    let mut text = String::new();
    for filed in &outcome.filed {
        writeln!(text, "Filed {} in {}", filed.title, filed.path).unwrap();
        if let Some(from) = &filed.expanded_from {
            let name = from.file_name().unwrap_or_default().to_string_lossy();
            match &filed.trash_problem {
                None => writeln!(text, "  expanded from {name}, which is now in the Trash"),
                Some(problem) => writeln!(
                    text,
                    "  expanded from {name}, which was left where it was: {problem}"
                ),
            }
            .unwrap();
        }
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
        .filter(|f| f.kept_original.is_some() && f.trash_problem.is_none())
        .collect();
    if !kept.is_empty() {
        text.push_str("\nThe originals were copied, not moved (ingest.originals is keep); delete them when you're ready:\n");
        for filed in kept {
            if let Some(path) = &filed.kept_original {
                writeln!(text, "  {}", path.display()).unwrap();
            }
        }
    }
    for problem in &outcome.listing_problems {
        writeln!(text, "warning: a listing couldn't be updated: {problem}").unwrap();
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
                "expanded_from": f.expanded_from,
                "trash_problem": f.trash_problem,
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

/// The editor to open: the `editor` preference, then `$VISUAL`, then `$EDITOR`, then
/// `notepad` on Windows or `vi`. Its words are split as a shell would, so `code --wait` works.
pub(crate) fn editor() -> (String, &'static str) {
    let preference = crate::config::Prefs::load().ok().and_then(|p| p.editor);
    carrel_core::file_pickers::editor_in_use(preference.as_deref())
}

/// Opens `path` in the editor and waits for it to close.
pub(crate) fn run_editor(path: &Path) -> Result<(), Failure> {
    let words = crate::paths::split_words(&editor().0);
    let (program, args) = words
        .split_first()
        .ok_or_else(|| Failure::new("no editor to open; set VISUAL or EDITOR"))?;
    let status = std::process::Command::new(program)
        .args(args)
        .arg(path)
        .status()
        .map_err(|e| Failure::new(format!("couldn't open the editor {program}: {e}")))?;
    if !status.success() {
        return Err(Failure::new(format!(
            "the editor {program} exited with {status}; nothing changed"
        )));
    }
    Ok(())
}

/// `carrel review edit ID`: opens the proposal's card in the editor, checks it when saved,
/// and reopens it with the problems listed until it checks out or is left unchanged.
pub fn edit(archive: Option<&Path>, id: &str) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let mut entry = actions::proposals(&archive, &[id.to_owned()])?
        .pop()
        .ok_or_else(|| Failure::new(format!("no proposal {id}")))?;
    let file = std::env::temp_dir().join(format!("carrel-{}.yaml", entry.id));
    let mut text = actions::draft_text(&archive, &entry, &[])?;
    let result = loop {
        std::fs::write(&file, &text).map_err(|e| Failure::io(&file, &e))?;
        run_editor(&file)?;
        let edited = std::fs::read_to_string(&file).map_err(|e| Failure::io(&file, &e))?;
        if edited == text {
            break "Nothing changed\n".to_owned();
        }
        match actions::apply_draft(&archive, &mut entry, &edited)? {
            Edited::Unchanged => break "Nothing changed\n".to_owned(),
            Edited::Changed => {
                ReviewQueue::of(&archive).save(&entry)?;
                break format!(
                    "Saved the card for {} ({})\n",
                    short_id(entry.id),
                    entry.state.as_str()
                );
            }
            Edited::Problems(problems) => {
                // The owner's edits stay; the problems go on top, replacing earlier ones.
                let mut again = String::new();
                for problem in &problems {
                    writeln!(again, "# To fix: {problem}").unwrap();
                }
                again.push_str("#\n");
                for line in edited
                    .lines()
                    .skip_while(|l| l.starts_with("# To fix: ") || *l == "#")
                {
                    again.push_str(line);
                    again.push('\n');
                }
                text = again;
            }
        }
    };
    let _ = std::fs::remove_file(&file);
    Ok(Report::new(
        result,
        serde_json::to_value(&entry).unwrap_or(Value::Null),
    ))
}
