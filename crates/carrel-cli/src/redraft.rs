//! `carrel review redraft`: drafting waiting proposals again with the models this device has
//! now, as when they were drafted while no text model could be reached (#445).

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::ingest::redraft::{RedraftError, failed_drafts, no_drafting_model, redraft};
use carrel_core::providers::Registry;
use carrel_core::review::short_id;
use serde_json::json;

use crate::commands::{holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// Why drafting can't run here, and what would let it: the model to download, or the
/// provider to put beside `carrel`.
pub(crate) fn no_model_words(registry: &Registry) -> String {
    let mut text = format!("{}.", RedraftError::NoTextModel);
    if let Some(missing) = no_drafting_model(
        registry,
        crate::ask::memory_gb(),
        carrel_core::local::models_dir().as_deref(),
    ) {
        write!(text, " {}", missing.explain_for("Drafting cards")).unwrap();
        if let Some(download) = missing.download {
            write!(
                text,
                " Download it with `carrel models download {}`, which asks first.",
                download.name
            )
            .unwrap();
        }
    }
    text
}

/// `carrel review redraft ID...`, or `--all-failed` for every waiting proposal drafted here
/// whose card or collection couldn't be settled.
pub fn command(
    archive: Option<&Path>,
    ids: &[String],
    all_failed: bool,
) -> Result<Report, Failure> {
    let mut display = crate::progress::Display::new("", false);
    let report = redraft_proposals(archive, ids, all_failed, &mut |p| {
        display.count(p.stage.describe(), p.done, p.total, p.item);
    });
    display.finish();
    report
}

/// [`command`], reporting each stage to `progress`, as the interactive mode does quietly.
pub(crate) fn redraft_proposals(
    archive: Option<&Path>,
    ids: &[String],
    all_failed: bool,
    progress: &mut dyn FnMut(carrel_core::ingest::run::Progress<'_>),
) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let device = holder(&Identity::load_or_create()?).device;
    let ids: Vec<String> = if all_failed {
        failed_drafts(&archive, device)?
            .iter()
            .map(|e| e.id.to_string())
            .collect()
    } else {
        ids.to_vec()
    };
    if ids.is_empty() {
        return Ok(Report::new(
            "No proposals need drafting again\n".to_owned(),
            json!({ "redrafted": [], "failed": [] }),
        ));
    }
    let (registry, _) = crate::providers::registry();
    let mut text = String::new();
    let mut redrafted = Vec::new();
    let mut failed = Vec::new();
    for id in &ids {
        let result = redraft(&archive, &registry, id, device, progress);
        match result {
            Ok(done) => {
                let entry = &done.entry;
                writeln!(
                    text,
                    "Drafted {} again: {} → {} ({} confidence)",
                    short_id(entry.id),
                    entry.card.title,
                    entry.destination.path,
                    format!("{:?}", entry.destination.confidence).to_lowercase()
                )
                .unwrap();
                if !done.kept.is_empty() {
                    writeln!(text, "  kept what you set: {}", done.kept.join(", ")).unwrap();
                }
                for problem in &entry.problems {
                    writeln!(text, "  to settle: {}: {}", problem.stage, problem.message).unwrap();
                }
                redrafted.push(json!({ "entry": entry, "kept": done.kept }));
            }
            Err(RedraftError::NoTextModel) => {
                return Err(Failure::new(no_model_words(&registry)));
            }
            Err(e) => {
                writeln!(text, "Not drafted again {id}: {e}").unwrap();
                failed.push(json!({ "id": id, "error": e.to_string() }));
            }
        }
    }
    if redrafted.is_empty() {
        return Err(Failure::new(text.trim_end().to_owned()));
    }
    Ok(Report::new(
        text,
        json!({ "redrafted": redrafted, "failed": failed }),
    ))
}
