//! `carrel ingest`: proposing to file new files and folders (`docs/design/ingest.md`). Each
//! new item is examined, its text read, its card drafted and its destination chosen, and the
//! result saved as a review proposal. Nothing is moved until a proposal is approved.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::actions::{self, Ingested};
use carrel_core::backup::Backups;
use carrel_core::card::{Id, Kind};
use carrel_core::ingest::card::Draft;
use carrel_core::ingest::filing::Outcome;
use carrel_core::ingest::route::Route;
use carrel_core::ingest::run::{Run, name};
use carrel_core::ingest::sensitivity::Assessment;
use carrel_core::ingest::text::ItemText;
use carrel_core::ingest::{Candidate, IntakeOptions};
use carrel_core::questions::QuestionEntry;
use carrel_core::review::short_id;
use serde_json::{Value, json};

use crate::commands::locate;
use crate::{Failure, Report};

/// `carrel ingest PATH...`.
pub fn ingest(
    archive: Option<&Path>,
    paths: &[PathBuf],
    options: IntakeOptions,
    dry_run: bool,
    json: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let (registry, _) = crate::providers::registry();
    let identity = crate::config::Identity::load_or_create()?;
    let (backups, _) = crate::backup::for_batch(&archive);
    let mut display = crate::progress::Display::new("", json);
    let Ingested { run, filed } = if dry_run {
        let run = carrel_core::ingest::run::run_with_progress(
            &archive,
            &registry,
            paths,
            options,
            identity.device,
            &mut |p| display.update(p),
        )?;
        Ingested { run, filed: None }
    } else {
        actions::ingest(
            &archive,
            &registry,
            paths,
            options,
            crate::commands::holder(&identity),
            backups.as_ref().map(|b| b as &dyn Backups),
            &mut |p| display.update(p),
        )?
    };
    display.finish();
    Ok(report(&run, filed.as_ref(), dry_run))
}

/// An ingest run as a report: the plan for people, and everything as JSON.
pub fn report(run: &Run, filed: Option<&Outcome>, dry_run: bool) -> Report {
    let mut out = render(
        &run.candidates,
        &run.texts,
        &run.drafts,
        &run.routes,
        &run.assessments,
        &run.skipped,
    );
    for problem in &run.routing_problems {
        writeln!(out, "warning: {problem}").unwrap();
    }
    let saved = run.entries.iter().flatten().count();
    closing(&mut out, filed, &run.questions, saved, dry_run, run.batch);

    let items = items_json(&run.candidates, &run.texts, &run.drafts);
    Report::new(
        out,
        json!({
            "batch": run.batch,
            "saved": !dry_run,
            "items": items,
            "skipped": run.skipped.iter().map(|s| json!({"path": s.path, "reason": s.reason})).collect::<Vec<_>>(),
            "proposals": run.entries.iter().flatten().collect::<Vec<_>>(),
            "filed": filed.map(|o| o.filed.iter().map(|f| json!({"proposal": f.entry, "item": f.item, "path": f.path, "card": f.card})).collect::<Vec<_>>()).unwrap_or_default(),
        }),
    )
}

/// What was filed by rule, what was asked, and what was saved for review.
fn closing(
    out: &mut String,
    auto_filed: Option<&Outcome>,
    questions: &[QuestionEntry],
    proposed: usize,
    dry_run: bool,
    batch: Id,
) {
    if !questions.is_empty() {
        out.push_str("\nQuestions (answer them with `carrel questions answer`):\n");
        for question in questions {
            writeln!(
                out,
                "  {}  {} ({} item{})",
                short_id(question.id),
                question.question,
                question.items.len(),
                if question.items.len() == 1 { "" } else { "s" }
            )
            .unwrap();
        }
    }
    closing_lines(out, auto_filed, proposed, dry_run, batch);
}

/// What was filed by rule, and how many proposals were saved.
fn closing_lines(
    out: &mut String,
    auto_filed: Option<&Outcome>,
    proposed: usize,
    dry_run: bool,
    batch: Id,
) {
    if let Some(outcome) = auto_filed {
        for filed in &outcome.filed {
            writeln!(
                out,
                "Filed {} in {} (a routing rule files it without review)",
                filed.title, filed.path
            )
            .unwrap();
        }
        for (entry, error) in &outcome.failed {
            writeln!(out, "Not filed {}: {error}", entry.card.title).unwrap();
        }
    }
    let saved = proposed - auto_filed.map_or(0, |o| o.filed.len());
    if dry_run {
        out.push_str("\nNothing has been saved (--dry-run).\n");
    } else if saved > 0 {
        writeln!(
            out,
            "\nSaved {saved} proposal{} (batch {}). Review them with `carrel review`; nothing moves until you approve.",
            if saved == 1 { "" } else { "s" },
            short_id(batch)
        )
        .unwrap();
    }
}

/// Each item as JSON.
fn items_json(
    candidates: &[Candidate],
    texts: &[Option<ItemText>],
    drafts: &[Option<Draft>],
) -> Vec<Value> {
    candidates
        .iter()
        .zip(texts)
        .zip(drafts)
        .map(|((c, t), d)| {
            json!({
                "path": c.path,
                "kind": c.kind,
                "sealed": c.sealed,
                "markers": c.markers,
                "checksum": c.checksum,
                "size": c.size,
                "duplicate_of": c.duplicate_of.as_ref().map(|d| json!({"id": d.id, "card": d.card})),
                "same_as": c.same_as.map(|i| &candidates[i].path),
                "text": t.as_ref().map(text_json),
                "card": d.as_ref().map(draft_json),
            })
        })
        .collect()
}

/// The plan as a report for people.
fn render(
    candidates: &[Candidate],
    texts: &[Option<ItemText>],
    drafts: &[Option<Draft>],
    routes: &[Option<Route>],
    assessments: &[Option<Assessment>],
    skipped: &[carrel_core::ingest::Skipped],
) -> String {
    let mut out = String::new();
    match candidates.len() {
        0 => out.push_str("Nothing to ingest\n"),
        1 => out.push_str("1 item to ingest:\n"),
        n => writeln!(out, "{n} items to ingest:").unwrap(),
    }
    let width = candidates
        .iter()
        .map(|c| name(c).chars().count())
        .max()
        .unwrap_or(0);
    for ((((candidate, item_text), card), route), assessment) in candidates
        .iter()
        .zip(texts)
        .zip(drafts)
        .zip(routes)
        .zip(assessments)
    {
        let mut notes = Vec::new();
        if let Some(duplicate) = &candidate.duplicate_of {
            notes.push(format!("duplicate of {}", duplicate.card));
        }
        if let Some(earlier) = candidate.same_as {
            notes.push(format!(
                "same as {} in this batch",
                name(&candidates[earlier])
            ));
        }
        writeln!(
            out,
            "  {:<width$}  {}, {}{}",
            name(candidate),
            describe_kind(candidate),
            human_size(candidate.size),
            if notes.is_empty() {
                String::new()
            } else {
                format!("  ({})", notes.join("; "))
            }
        )
        .unwrap();
        if let Some(item_text) = item_text {
            writeln!(out, "  {:<width$}  {}", "", describe_text(item_text)).unwrap();
        }
        if let Some(card) = card {
            for line in describe_draft(card) {
                writeln!(out, "  {:<width$}  {line}", "").unwrap();
            }
        }
        if let Some(route) = route {
            let why = route
                .reasons
                .first()
                .map(|r| format!(": {r}"))
                .unwrap_or_default();
            writeln!(
                out,
                "  {:<width$}  to: {} ({} confidence){why}",
                "",
                route.path,
                format!("{:?}", route.confidence).to_lowercase()
            )
            .unwrap();
        }
        if let Some(assessment) = assessment {
            let level = serde_json::to_value(assessment.level)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            let why = if assessment.reasons.is_empty() {
                String::new()
            } else {
                format!(": {}", assessment.reasons.join("; "))
            };
            writeln!(out, "  {:<width$}  sensitivity: {level}{why}", "").unwrap();
        }
    }
    if !skipped.is_empty() {
        out.push_str("Skipped:\n");
        for skip in skipped {
            let name = skip.path.file_name().map_or_else(
                || skip.path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            writeln!(out, "  {name}: {}", skip.reason).unwrap();
        }
    }
    out
}

/// The drafted card, a few lines for people.
fn describe_draft(draft: &Draft) -> Vec<String> {
    let mut lines = vec![format!("card: \u{201c}{}\u{201d}", draft.title)];
    if let Some(summary) = &draft.summary {
        lines.push(format!("      {summary}"));
    }
    let mut tags: Vec<String> = draft.tags.iter().map(|t| t.0.clone()).collect();
    if let Some(new_tag) = &draft.new_tag {
        tags.push(format!("{} (new)", new_tag.0));
    }
    if !tags.is_empty() {
        lines.push(format!("      tags: {}", tags.join(", ")));
    }
    if let Some(model) = draft.generated.as_ref().and_then(|g| g.model.as_ref()) {
        lines.push(format!("      drafted by {model}"));
    }
    for problem in &draft.problems {
        lines.push(format!("      to settle: {problem}"));
    }
    lines
}

fn draft_json(draft: &Draft) -> Value {
    json!({
        "title": draft.title,
        "summary": draft.summary,
        "tags": draft.tags,
        "new_tag": draft.new_tag,
        "created": draft.created,
        "generated": draft.generated,
        "problems": draft.problems,
        "attempts": draft.attempts,
    })
}

/// The provider and offer that read the text, if one did.
fn reader(text: &ItemText) -> Option<String> {
    text.attempts
        .iter()
        .rfind(|a| a.result == "ok")
        .map(|a| format!("{}/{}", a.provider, a.offer))
}

/// `text: 3200 characters, embedded, from apple/pdfkit (12 pages)`.
fn describe_text(text: &ItemText) -> String {
    if text.method == "none" {
        return format!(
            "no text: {}",
            text.note.as_deref().unwrap_or("nothing could read it")
        );
    }
    let mut line = format!(
        "text: {} characters, {}",
        text.text.chars().count(),
        text.method
    );
    if let Some(reader) = reader(text) {
        write!(line, ", from {reader}").unwrap();
    }
    if let Some(pages) = text.pages {
        write!(line, " ({pages} page{})", if pages == 1 { "" } else { "s" }).unwrap();
    }
    line
}

fn text_json(text: &ItemText) -> Value {
    json!({
        "method": text.method,
        "media_type": text.media_type,
        "characters": text.text.chars().count(),
        "pages": text.pages,
        "read_by": reader(text),
        "note": text.note,
    })
}

fn describe_kind(candidate: &Candidate) -> String {
    match candidate.kind {
        Kind::Bundle if !candidate.markers.is_empty() => {
            format!("bundle (sealed by {})", candidate.markers.join(", "))
        }
        Kind::Bundle => "bundle".into(),
        Kind::Folder => "folder".into(),
        _ => "file".into(),
    }
}

/// `1.3 KB`, `482 KB`, `2.1 GB`.
pub(crate) fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    #[allow(clippy::cast_precision_loss)] // a display value
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_read_naturally() {
        assert_eq!(super::human_size(0), "0 B");
        assert_eq!(super::human_size(999), "999 B");
        assert_eq!(super::human_size(1294), "1.3 KB");
        assert_eq!(super::human_size(482_113), "482 KB");
        assert_eq!(super::human_size(2_100_000_000), "2.1 GB");
    }
}
