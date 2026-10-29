//! `carrel ingest`: putting new files and folders into the archive. For now it examines them
//! and reports the plan (kind, size, duplicates, and how each item's text was read) without
//! changing anything; drafting cards and destinations follows (`docs/design/ingest.md`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::card::Kind;
use carrel_core::ingest::card::{Context, Draft, Glossary, Vocabulary, draft};
use carrel_core::ingest::text::{ItemText, read};
use carrel_core::ingest::{Candidate, IntakeOptions, plan};
use serde_json::{Value, json};

use crate::commands::locate;
use crate::{Failure, Report};

/// `carrel ingest PATH...`.
pub fn ingest(
    archive: Option<&Path>,
    paths: &[PathBuf],
    options: IntakeOptions,
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

    let out = render(&candidates, &texts, &drafts, &skipped);

    let items: Vec<Value> = candidates
        .iter()
        .zip(&texts)
        .zip(&drafts)
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
        .collect();
    let skipped: Vec<Value> = skipped
        .iter()
        .map(|s| json!({"path": s.path, "reason": s.reason}))
        .collect();
    Ok(Report::new(
        out,
        json!({ "items": items, "skipped": skipped }),
    ))
}

/// The plan as a report for people.
fn render(
    candidates: &[Candidate],
    texts: &[Option<ItemText>],
    drafts: &[Option<Draft>],
    skipped: &[carrel_core::ingest::Skipped],
) -> String {
    let name = |c: &Candidate| {
        c.path.file_name().map_or_else(
            || c.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
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
    for ((candidate, item_text), card) in candidates.iter().zip(texts).zip(drafts) {
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
    if !candidates.is_empty() {
        out.push_str("\nNothing has been changed. Choosing destinations, and saving these as proposals, comes next.\n");
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
