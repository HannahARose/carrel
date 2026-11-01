//! `carrel bench --restructure`: the restructure benchmark (`docs/design/restructure.md`,
//! "Measuring it"). It files the test set into the muddled copy of its archive the crawler
//! benchmark uses (`muddle.yaml`), embeds its cards for grouping, then has Carrel propose a
//! restructure of the whole archive at each eagerness, with numbers alone and with each text
//! model, and scores whether each proposal recovers the intended shape, how many moves it
//! takes and its findability gain. It writes `results.json` and `report.md` to `restructure/`
//! in the output folder. The scoring is in the core ([`carrel_core::bench::restructure`]).
//!
//! Each proposal's score is saved to `restructure/runs/` as soon as it's made, so a stopped run
//! carries on where it stopped; `--fresh` starts over.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use carrel_core::archive::Archive;
use carrel_core::bench::crawl::{Muddle, build};
use carrel_core::bench::restructure::{RestructureScore, at_eagerness, score};
use carrel_core::bench::{Answers, check, percent};
use carrel_core::card::Id;
use carrel_core::discover::Catalog;
use carrel_core::graph::Graph;
use carrel_core::index::Index;
use carrel_core::providers::{Capability, Registry};
use carrel_core::restructure::eagerness::Eagerness;
use carrel_core::restructure::grouping::card_vectors;
use carrel_core::restructure::section::{SectionOptions, propose_section};
use serde_json::{Value, json};

use crate::bench::{BenchArgs, Model, choose, file_safe};
use crate::{Failure, Report};

/// One proposal, scored: who worded it (none for numbers alone) and at what eagerness.
struct Row {
    model: Option<Model>,
    eagerness: Eagerness,
    score: RestructureScore,
}

/// `carrel bench --restructure`.
///
/// # Errors
///
/// [`Failure`] if the test set or its muddle are inconsistent, a model asked for isn't
/// available, or the archive, its index or the results can't be written.
pub fn restructure_bench(args: &BenchArgs, json: bool) -> Result<Report, Failure> {
    let corpus = args.corpus.as_path();
    let (answers, muddle) = load(corpus)?;
    let dir = args.out.join("restructure");
    if args.fresh {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let archive = build(
        &corpus.join("archive"),
        &corpus.join("items"),
        &answers,
        &muddle,
        &dir.join("archive"),
    )
    .map_err(Failure::new)?;
    let mut registry = crate::providers::registry().0;
    crate::search::read_texts(&archive, &registry, json)?;
    let mut notes = Vec::new();
    let vectors = vectors(&archive, &registry, json, &mut notes)?;
    let models = choose(&registry, &args.models)?;
    let files: BTreeMap<Id, String> = archive
        .items()?
        .into_iter()
        .filter_map(|i| Some((i.id, i.item?.rsplit('/').next()?.to_owned())))
        .collect();
    let places = muddle.places(&answers);
    let runs = dir.join("runs");
    std::fs::create_dir_all(&runs).map_err(|e| Failure::io(&runs, &e))?;

    let mut rows = Vec::new();
    let wordings: Vec<Option<Model>> = std::iter::once(None).chain(models.into_iter().map(Some)).collect();
    for model in wordings {
        let saved_path = runs.join(match &model {
            Some(m) => format!("{}--{}.json", file_safe(&m.provider), file_safe(&m.offer)),
            None => "numbers-only.json".to_owned(),
        });
        let mut saved = load_saved(&saved_path);
        if let Some(m) = &model {
            registry.pin(Capability::TextModel, &m.provider, &m.offer);
        }
        for eagerness in Eagerness::ALL {
            let key = eagerness.as_str();
            if !saved.contains_key(key) {
                let archive = at_eagerness(&archive, eagerness).map_err(Failure::new)?;
                let started = Instant::now();
                let proposal = propose_section(
                    &archive,
                    "",
                    &vectors,
                    model.as_ref().map(|_| &registry),
                    &SectionOptions::for_archive(&archive),
                )?;
                let scored = score(
                    &proposal,
                    &files,
                    &places,
                    &answers,
                    started.elapsed().as_secs_f64(),
                );
                saved.insert(key.to_owned(), scored);
                let text = serde_json::to_string_pretty(&saved).expect("scores serialize") + "\n";
                std::fs::write(&saved_path, text).map_err(|e| Failure::io(&saved_path, &e))?;
            }
            rows.push(Row {
                model: model.clone(),
                eagerness,
                score: saved[key].clone(),
            });
        }
    }
    at_eagerness(&archive, Eagerness::default()).map_err(Failure::new)?;

    let results = results_json(&rows, &notes);
    let results_path = dir.join("results.json");
    std::fs::write(&results_path, format!("{results:#}\n"))
        .map_err(|e| Failure::io(&results_path, &e))?;
    let report_path = dir.join("report.md");
    std::fs::write(&report_path, report_markdown(&rows, &notes))
        .map_err(|e| Failure::io(&report_path, &e))?;
    let mut text = summary_table(&rows);
    for note in &notes {
        writeln!(text, "\n{note}").unwrap();
    }
    writeln!(
        text,
        "\nWrote {} and {}.",
        results_path.display(),
        report_path.display()
    )
    .unwrap();
    Ok(Report::new(text, results))
}

/// The answers and the muddle in `corpus`, once both are known to agree with the test set.
fn load(corpus: &Path) -> Result<(Answers, Muddle), Failure> {
    let answers = Answers::load(&corpus.join("answers.yaml")).map_err(Failure::new)?;
    let muddle = Muddle::load(&corpus.join("muddle.yaml")).map_err(Failure::new)?;
    let archive = Archive::open(&corpus.join("archive"))?;
    let mut problems = check(&answers, &archive, &corpus.join("items")).map_err(Failure::new)?;
    problems.extend(muddle.check(&answers, &archive));
    if problems.is_empty() {
        Ok((answers, muddle))
    } else {
        Err(Failure::new(format!(
            "the test set is inconsistent:\n  {}",
            problems.join("\n  ")
        )))
    }
}

/// The muddled archive's card vectors, for grouping: from the catalog's default embedding
/// model when a provider here runs it, else the first one on offer, its index kept in the
/// archive and caught up. Without one, the items are grouped by the tag and link graph alone,
/// and `notes` says so.
fn vectors(
    archive: &Archive,
    registry: &Registry,
    json: bool,
    notes: &mut Vec<String>,
) -> Result<std::collections::HashMap<Id, Vec<f32>>, Failure> {
    let offered = carrel_core::index::manage::offered_models(registry);
    let Some(model) = offered
        .iter()
        .find(|m| m.model == carrel_core::models::DEFAULT_EMBEDDING_MODEL)
        .or_else(|| offered.first())
        .map(|m| m.model.clone())
    else {
        notes.push(
            "No embedding model is on offer, so items were grouped by the tag and link graph alone."
                .into(),
        );
        return Ok(std::collections::HashMap::new());
    };
    let existing = Index::open(archive, &model).map_err(|e| crate::index::failure(e, &model))?;
    let mut index = existing.unwrap_or_else(|| Index::new(archive, &model));
    crate::index::catch_up(archive, registry, &mut index, json)?;
    notes.push(format!("Items were grouped by their cards' {model} vectors and the tag and link graph."));
    Ok(card_vectors(&index, &Graph::build(&Catalog::read(archive)?), ""))
}

/// The scores saved by an earlier run for one model, by eagerness.
fn load_saved(path: &Path) -> BTreeMap<String, RestructureScore> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn label(model: Option<&Model>) -> &str {
    model.map_or("numbers alone", |m| m.label.as_str())
}

fn results_json(rows: &[Row], notes: &[String]) -> Value {
    json!({
        "date": jiff::Zoned::now().date().to_string(),
        "carrel": carrel_core::CARREL_VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "notes": notes,
        "proposals": rows.iter().map(|r| json!({
            "provider": r.model.as_ref().map(|m| &m.provider),
            "offer": r.model.as_ref().map(|m| &m.offer),
            "label": label(r.model.as_ref()),
            "version": r.model.as_ref().and_then(|m| m.version.as_ref()),
            "eagerness": r.eagerness.as_str(),
            "score": r.score,
        })).collect::<Vec<_>>(),
    })
}

/// One row per proposal.
fn summary_table(rows: &[Row]) -> String {
    let mut text = String::from(
        "| Wording | Eagerness | Changes | Shape before → after | Fixed | Broken | Moves (weighted) | Findability gain | s |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for r in rows {
        let s = &r.score;
        let gain = s.findability_gain.map_or_else(
            || "untested".to_owned(),
            |g| format!("{:+.0} points", g * 100.0),
        );
        writeln!(
            text,
            "| {} | {} | {} | {} → {} | {} of {} | {} | {} ({}) | {gain} | {:.1} |",
            label(r.model.as_ref()),
            r.eagerness.as_str(),
            s.changes,
            percent(s.before.f1),
            percent(s.after.f1),
            s.fixed,
            s.misplaced,
            s.broken,
            s.moves.moved,
            s.moves.weighted,
            s.seconds
        )
        .unwrap();
    }
    text
}

/// The report: the summary, how to read it, then each proposal's moves by invasiveness.
fn report_markdown(rows: &[Row], notes: &[String]) -> String {
    let mut text = format!(
        "# Restructure benchmark report\n\n{} on {} {}, carrel {}: a whole-archive proposal for the muddled test archive at each eagerness, with numbers alone and with each text model.\n\n## Summary\n\n",
        jiff::Zoned::now().date(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        carrel_core::CARREL_VERSION,
    );
    text.push_str(&summary_table(rows));
    text.push_str(
        "\n**Shape** is how closely the items that end up together match the intended shape (the pairs' F1), before and after the proposal; it recovers the shape when it rises. **Fixed** is the misplaced items it puts in an acceptable collection or one inside it, and **broken** the well-placed items it moves out of one. **Moves** are items moved, and weighted by each change's invasiveness (1 for rewriting to 5 for top-level collections). **Findability gain** is its own test's held-out cards found in the proposed tree less those found in the current one; numbers alone can't run it.\n",
    );
    for note in notes {
        writeln!(text, "\n{note}").unwrap();
    }
    for r in rows {
        writeln!(
            text,
            "\n## {}, {}\n",
            label(r.model.as_ref()),
            r.eagerness.as_str()
        )
        .unwrap();
        let s = &r.score;
        if s.changes == 0 {
            text.push_str("No changes proposed.\n");
        }
        for (kind, moved) in s.moves.by_invasiveness.iter().filter(|(_, n)| **n > 0) {
            writeln!(text, "- {}: {moved} items", kind.describe()).unwrap();
        }
        if let Some(per) = s.weighted_moves_per_fix {
            writeln!(text, "- {per:.1} weighted moves for each item it fixes").unwrap();
        }
        if let Some((tried, now, after)) = s.findability {
            writeln!(text, "- Findability: {now} of {tried} found now, {after} after").unwrap();
        }
        if let Some(why) = &s.untested {
            writeln!(text, "- Untested: {why}").unwrap();
        }
    }
    text
}
