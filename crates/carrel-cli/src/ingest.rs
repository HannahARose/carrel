//! `carrel ingest`: putting new files and folders into the archive. For now it examines them
//! and reports the plan (kind, size, duplicates) without changing anything; drafting cards
//! and destinations follows (`docs/design/ingest.md`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::card::Kind;
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

    let name = |c: &Candidate| {
        c.path.file_name().map_or_else(
            || c.path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        )
    };
    let mut text = String::new();
    match candidates.len() {
        0 => text.push_str("Nothing to ingest\n"),
        1 => text.push_str("1 item to ingest:\n"),
        n => writeln!(text, "{n} items to ingest:").unwrap(),
    }
    let width = candidates
        .iter()
        .map(|c| name(c).chars().count())
        .max()
        .unwrap_or(0);
    for candidate in &candidates {
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
            text,
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
    }
    if !skipped.is_empty() {
        text.push_str("Skipped:\n");
        for skip in &skipped {
            let name = skip.path.file_name().map_or_else(
                || skip.path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            writeln!(text, "  {name}: {}", skip.reason).unwrap();
        }
    }
    if !candidates.is_empty() {
        text.push_str("\nNothing has been changed. Drafting cards and destinations comes next.\n");
    }

    let items: Vec<Value> = candidates
        .iter()
        .map(|c| {
            json!({
                "path": c.path,
                "kind": c.kind,
                "sealed": c.sealed,
                "markers": c.markers,
                "checksum": c.checksum,
                "size": c.size,
                "duplicate_of": c.duplicate_of.as_ref().map(|d| json!({"id": d.id, "card": d.card})),
                "same_as": c.same_as.map(|i| &candidates[i].path),
            })
        })
        .collect();
    let skipped: Vec<Value> = skipped
        .iter()
        .map(|s| json!({"path": s.path, "reason": s.reason}))
        .collect();
    Ok(Report::new(
        text,
        json!({ "items": items, "skipped": skipped }),
    ))
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
