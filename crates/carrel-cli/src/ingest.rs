//! `carrel ingest`: proposing to file new files and folders (`docs/design/ingest.md`). Each
//! new item is examined, its text read, its card drafted and its destination chosen, and the
//! result saved as a review proposal. Nothing is moved until a proposal is approved.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::card::Id;
use carrel_core::card::Kind;
use carrel_core::coordinator::{Coordinator, DEFAULT_LEASE, DiskLock};
use carrel_core::ingest::card::{Context, Draft, Glossary, Vocabulary, draft};
use carrel_core::ingest::filing::{Outcome, Placement, file_batch, files_itself};
use carrel_core::ingest::questions::ask;
use carrel_core::ingest::route::{Route, Router, RoutingRules};
use carrel_core::ingest::sensitivity::{Assessment, assess};
use carrel_core::ingest::text::{ItemText, read};
use carrel_core::ingest::{Batch, Candidate, IntakeOptions, plan};
use carrel_core::providers::Registry;
use carrel_core::questions::{QuestionEntry, QuestionQueue, mark_waiting};
use carrel_core::review::{ReviewEntry, ReviewQueue, short_id};
use serde_json::{Value, json};

use crate::commands::locate;
use crate::{Failure, Report};

/// `carrel ingest PATH...`.
pub fn ingest(
    archive: Option<&Path>,
    paths: &[PathBuf],
    options: IntakeOptions,
    dry_run: bool,
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let (candidates, skipped) = plan(&archive, paths, options)?;
    let (registry, _) = crate::providers::registry();
    let rules = archive.rules()?;
    // Duplicates won't be filed by default, so their text isn't needed yet.
    let identity = crate::config::Identity::load_or_create()?;
    let collections = archive.collections()?;
    let vocabulary = Vocabulary::of(&archive, &collections);
    let glossary = Glossary::of(&archive);
    let context = Context {
        archive_name: &archive.doc().name,
        vocabulary: &vocabulary,
        glossary: &glossary,
        device: identity.device,
    };
    let texts: Vec<Option<ItemText>> = candidates
        .iter()
        .map(|c| {
            (c.duplicate_of.is_none() && c.same_as.is_none()).then(|| read(&registry, &rules, c))
        })
        .collect();
    let drafts: Vec<Option<Draft>> = candidates
        .iter()
        .zip(&texts)
        .map(|(c, t)| t.as_ref().map(|t| draft(&registry, &context, c, t)))
        .collect();

    let routing_rules = RoutingRules::of(&archive, &collections);
    let chooser = Router::new(&archive, &collections, &routing_rules);
    let routes: Vec<Option<Route>> = candidates
        .iter()
        .zip(&drafts)
        .map(|(c, d)| {
            d.as_ref().map(|d| {
                let source = c.path.parent().unwrap_or(&c.path);
                chooser.route(&registry, c, source, d)
            })
        })
        .collect();
    let batch = Batch {
        archive: &archive,
        collections: &collections,
        id: Id::new_v7(),
        device: identity.device,
    };
    let assessments = assess_all(&registry, &batch, &candidates, &texts, &routes);
    let mut entries: Vec<Option<ReviewEntry>> = candidates
        .iter()
        .zip(
            texts
                .iter()
                .zip(&drafts)
                .zip(routes.iter().zip(&assessments)),
        )
        .map(|(c, ((t, d), (r, s)))| match (t, d, r, s) {
            (Some(t), Some(d), Some(r), Some(s)) => batch.propose(c, d, r, s, &t.attempts),
            _ => None,
        })
        .collect();
    let questions = ask_about(&archive, &collections, &glossary, &mut entries);
    let mut auto_filed = None;
    if !dry_run {
        save(&archive, &entries, &questions)?;
        auto_filed = file_by_rule(&archive, &entries, &routes, identity.device)?;
    }

    let mut out = render(
        &candidates,
        &texts,
        &drafts,
        &routes,
        &assessments,
        &skipped,
    );
    for problem in &routing_rules.problems {
        writeln!(out, "warning: {problem}").unwrap();
    }
    let saved = entries.iter().flatten().count();
    closing(
        &mut out,
        auto_filed.as_ref(),
        &questions,
        saved,
        dry_run,
        batch.id,
    );

    let items = items_json(&candidates, &texts, &drafts);
    Ok(Report::new(
        out,
        json!({
            "batch": batch.id,
            "saved": !dry_run,
            "items": items,
            "skipped": skipped.iter().map(|s| json!({"path": s.path, "reason": s.reason})).collect::<Vec<_>>(),
            "proposals": entries.iter().flatten().collect::<Vec<_>>(),
            "filed": auto_filed.as_ref().map(|o| o.filed.iter().map(|f| json!({"proposal": f.entry, "item": f.item, "path": f.path, "card": f.card})).collect::<Vec<_>>()).unwrap_or_default(),
        }),
    ))
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

/// Saves the batch's proposals and questions.
fn save(
    archive: &Archive,
    entries: &[Option<ReviewEntry>],
    questions: &[QuestionEntry],
) -> Result<(), Failure> {
    let queue = ReviewQueue::of(archive);
    for entry in entries.iter().flatten() {
        queue.save(entry)?;
    }
    let asked = QuestionQueue::of(archive);
    for question in questions {
        asked.save(question)?;
    }
    Ok(())
}

/// The questions the batch raises, with the proposals they name marked as waiting.
fn ask_about(
    archive: &Archive,
    collections: &[carrel_core::archive::Collection],
    glossary: &Glossary,
    entries: &mut [Option<ReviewEntry>],
) -> Vec<QuestionEntry> {
    let proposed: Vec<ReviewEntry> = entries.iter().flatten().cloned().collect();
    let questions = ask(archive, collections, glossary, &proposed);
    for entry in entries.iter_mut().flatten() {
        mark_waiting(entry, &questions);
    }
    questions
}

/// Files the proposals an owner rule marks `auto_file`, holding the writer lease.
fn file_by_rule(
    archive: &Archive,
    entries: &[Option<ReviewEntry>],
    routes: &[Option<Route>],
    device: Id,
) -> Result<Option<Outcome>, Failure> {
    let chosen: Vec<(ReviewEntry, Placement)> = entries
        .iter()
        .zip(routes)
        .filter_map(|(e, r)| match (e, r) {
            (Some(e), Some(r)) if files_itself(e, r) => Some((e.clone(), Placement::Destination)),
            _ => None,
        })
        .collect();
    if chosen.is_empty() {
        return Ok(None);
    }
    let identity = crate::config::Identity::load_or_create()?;
    let mut lease = DiskLock::for_archive(archive).acquire(
        crate::commands::holder(&identity),
        "ingest",
        DEFAULT_LEASE,
    )?;
    let (backups, _) = crate::backup::for_batch(archive);
    let outcome = file_batch(
        archive,
        chosen,
        &mut lease,
        device,
        backups
            .as_ref()
            .map(|b| b as &dyn carrel_core::backup::Backups),
    );
    lease.release()?;
    Ok(Some(outcome?))
}

/// How sensitive each item that has text and a destination is.
fn assess_all(
    registry: &Registry,
    batch: &Batch,
    candidates: &[Candidate],
    texts: &[Option<ItemText>],
    routes: &[Option<Route>],
) -> Vec<Option<Assessment>> {
    candidates
        .iter()
        .zip(texts.iter().zip(routes))
        .map(|(c, (t, r))| match (t, r) {
            (Some(t), Some(r)) => Some(assess(
                registry,
                &name(c),
                &t.text,
                batch.default_sensitivity(r),
            )),
            _ => None,
        })
        .collect()
}

/// An item's file or folder name.
fn name(candidate: &Candidate) -> String {
    candidate.path.file_name().map_or_else(
        || candidate.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
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
fn human_size(bytes: u64) -> String {
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
