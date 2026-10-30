//! `carrel bench`: the model benchmark (`docs/design/benchmark.md`). It ingests the test set
//! with each model in turn, scores what each proposed, and writes `results.json` and a
//! side-by-side `report.md`. It's for people developing Carrel and choosing models, so it's
//! left out of `carrel --help`.
//!
//! Items are ingested one at a time, and each result is saved to `runs/` in the output folder
//! as soon as it's done. Stopping a run (Ctrl-C) loses at most the item in progress, and
//! running again with the same `--out` carries on where it stopped; `--fresh` starts over.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use carrel_core::archive::Archive;
use carrel_core::bench::{
    Answers, BAR, ItemScore, Outcome, Rating, Score, check, outcomes, percent, score,
};
use carrel_core::card::Id;
use carrel_core::ingest::IntakeOptions;
use carrel_core::ingest::run::run_with_progress;
use carrel_core::providers::{Capability, Registry};
use serde_json::{Value, json};

use crate::{Failure, Report};

/// A model to measure: a provider's text-model offer.
struct Model {
    provider: String,
    offer: String,
    label: String,
    version: Option<String>,
}

/// One model's results.
struct Measured {
    model: Model,
    score: Score,
    items: Vec<(Outcome, ItemScore)>,
}

/// `carrel bench`.
///
/// # Errors
///
/// [`Failure`] if the test set is inconsistent, a model asked for isn't available, or the
/// results can't be written.
pub fn bench(
    corpus: &Path,
    wanted: &[String],
    out: &Path,
    limit: Option<usize>,
    fresh: bool,
    json: bool,
) -> Result<Report, Failure> {
    let answers = Answers::load(&corpus.join("answers.yaml")).map_err(Failure::new)?;
    let fixture = corpus.join("archive");
    let items_dir = corpus.join("items");
    let problems = check(&answers, &Archive::open(&fixture)?, &items_dir).map_err(Failure::new)?;
    if !problems.is_empty() {
        return Err(Failure::new(format!(
            "the test set is inconsistent:\n  {}",
            problems.join("\n  ")
        )));
    }
    let mut items: Vec<PathBuf> = answers.items.keys().map(|f| items_dir.join(f)).collect();
    if let Some(limit) = limit {
        items.truncate(limit);
    }

    let runs = out.join("runs");
    if fresh {
        let _ = std::fs::remove_dir_all(&runs);
    }
    std::fs::create_dir_all(&runs).map_err(|e| Failure::io(&runs, &e))?;

    let (mut registry, _) = crate::providers::registry();
    let models = choose(&registry, wanted)?;
    let mut measured = Vec::new();
    let count = models.len();
    for (n, model) in models.into_iter().enumerate() {
        let saved_path = runs.join(format!(
            "{}--{}.jsonl",
            file_safe(&model.provider),
            file_safe(&model.offer)
        ));
        let mut saved = load_saved(&saved_path);
        let mut display =
            crate::progress::Display::new(format!("{} ({} of {count})", model.label, n + 1), json);
        let todo: Vec<&PathBuf> = items
            .iter()
            .filter(|p| {
                !saved
                    .iter()
                    .any(|(o, _)| Some(o.item.as_str()) == file_name(p))
            })
            .collect();
        if !todo.is_empty() {
            registry.pin(Capability::TextModel, &model.provider, &model.offer);
            let copy = std::env::temp_dir().join(format!("carrel-bench-{}", Id::new_v7()));
            copy_dir(&fixture, &copy).map_err(|e| Failure::io(&copy, &e))?;
            let result = measure(
                &copy,
                &registry,
                &todo,
                &saved_path,
                &mut saved,
                |done, item| {
                    display.count("items", items.len() - todo.len() + done, items.len(), item);
                },
            );
            let _ = std::fs::remove_dir_all(&copy);
            result?;
        }
        display.finish();
        let seconds: f64 = saved.iter().map(|(_, s)| s).sum();
        let found: Vec<Outcome> = saved.into_iter().map(|(o, _)| o).collect();
        let (score, scores) = score(&answers, &found, seconds);
        let mut by_item: Vec<(Outcome, ItemScore)> = Vec::new();
        for s in scores {
            if let Some(o) = found.iter().find(|o| o.item == s.item) {
                by_item.push((o.clone(), s));
            }
        }
        measured.push(Measured {
            model,
            score,
            items: by_item,
        });
    }

    std::fs::create_dir_all(out).map_err(|e| Failure::io(out, &e))?;
    let results = results_json(&measured, &answers);
    let results_path = out.join("results.json");
    std::fs::write(&results_path, format!("{results:#}\n"))
        .map_err(|e| Failure::io(&results_path, &e))?;
    let report_path = out.join("report.md");
    std::fs::write(&report_path, report_markdown(&measured, &answers))
        .map_err(|e| Failure::io(&report_path, &e))?;

    let mut text = summary_table(&measured);
    writeln!(
        text,
        "\nWrote {} and {}.",
        results_path.display(),
        report_path.display()
    )
    .unwrap();
    Ok(Report::new(text, results))
}

/// Ingests each of `todo` on its own with the pinned model, appending each outcome and its
/// time to `saved_path` as soon as it's known.
fn measure(
    archive: &Path,
    registry: &Registry,
    todo: &[&PathBuf],
    saved_path: &Path,
    saved: &mut Vec<(Outcome, f64)>,
    mut progress: impl FnMut(usize, Option<&str>),
) -> Result<(), Failure> {
    let archive = Archive::open(archive)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(saved_path)
        .map_err(|e| Failure::io(saved_path, &e))?;
    for (done, path) in todo.iter().enumerate() {
        progress(done, file_name(path));
        let started = Instant::now();
        let run = run_with_progress(
            &archive,
            registry,
            std::slice::from_ref(*path),
            IntakeOptions::default(),
            Id::new_v7(),
            &mut |_| {},
        )?;
        let seconds = started.elapsed().as_secs_f64();
        for outcome in outcomes(&run) {
            // One line per item, written whole, so a stop mid-run loses at most this item.
            let line = json!({ "outcome": outcome, "seconds": seconds });
            writeln!(file, "{line}").map_err(|e| Failure::io(saved_path, &e))?;
            saved.push((outcome, seconds));
        }
    }
    progress(todo.len(), None);
    Ok(())
}

/// The outcomes saved by an earlier run, skipping a line cut short when it was stopped.
fn load_saved(path: &Path) -> Vec<(Outcome, f64)> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            let outcome = serde_json::from_value(value.get("outcome")?.clone()).ok()?;
            Some((outcome, value.get("seconds")?.as_f64()?))
        })
        .collect()
}

fn file_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(|n| n.to_str())
}

/// A name usable as a file name everywhere.
fn file_safe(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// The models to measure: those named (as `provider/offer`, or an offer alone), or every
/// available text model.
fn choose(registry: &Registry, wanted: &[String]) -> Result<Vec<Model>, Failure> {
    let available: Vec<Model> = registry
        .ranked(Capability::TextModel)
        .into_iter()
        .map(|(provider, offer)| Model {
            provider: provider.name().to_owned(),
            offer: offer.id.clone(),
            label: format!(
                "{}/{}",
                provider.name(),
                offer.label.as_deref().unwrap_or(&offer.id)
            ),
            version: provider.program().and_then(|p| p.version.clone()),
        })
        .collect();
    if available.is_empty() {
        return Err(Failure::new(
            "no text model is available; see `carrel providers` and `carrel models`",
        ));
    }
    if wanted.is_empty() {
        return Ok(available);
    }
    let mut chosen = Vec::new();
    for name in wanted {
        let (provider, offer) = name
            .split_once('/')
            .map_or((None, name.as_str()), |(p, o)| (Some(p), o));
        let pick = available
            .iter()
            .find(|m| m.offer == offer && provider.is_none_or(|p| p == m.provider))
            .ok_or_else(|| {
                Failure::new(format!(
                    "no available text model {name}; available: {}",
                    available
                        .iter()
                        .map(|m| format!("{}/{}", m.provider, m.offer))
                        .collect::<Vec<_>>()
                        .join(", ")
                ))
            })?;
        chosen.push(Model {
            provider: pick.provider.clone(),
            offer: pick.offer.clone(),
            label: pick.label.clone(),
            version: pick.version.clone(),
        });
    }
    Ok(chosen)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

fn results_json(measured: &[Measured], answers: &Answers) -> Value {
    json!({
        "carrel": carrel_core::CARREL_VERSION,
        "date": jiff::Zoned::now().date().to_string(),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "bar": {
            "valid": BAR.valid,
            "routing": BAR.routing,
            "under_rated_restricted": BAR.under_rated_restricted,
            "under_rated": BAR.under_rated,
            "seconds_per_item": BAR.seconds_per_item,
        },
        "models": measured.iter().map(|m| json!({
            "provider": m.model.provider,
            "offer": m.model.offer,
            "label": m.model.label,
            "version": m.model.version,
            "score": m.score,
            "shortfalls": m.score.shortfalls(&BAR),
            "items": m.items.iter().map(|(o, s)| json!({
                "outcome": o,
                "score": s,
                "expected": answers.items.get(&o.item).map(|a| json!({
                    "destinations": a.destinations,
                    "sensitivity": a.sensitivity,
                    "date": a.date,
                    "tags": a.tags,
                })),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// The summary: one row per model, and whether it meets the bar.
fn summary_table(measured: &[Measured]) -> String {
    let mut text = String::from(
        "| Model | Valid | First try | Declined | Routing | Sensitivity exact | Under-rated | Restricted under | Dates | Tags found | Extra tags | s/item | Meets the bar |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for m in measured {
        let s = &m.score;
        let short = s.shortfalls(&BAR);
        writeln!(
            text,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {:.1} | {} |",
            m.model.label,
            percent(s.valid),
            percent(s.first_try),
            percent(s.declined),
            percent(s.routing),
            percent(s.sensitivity_exact),
            percent(s.under_rated),
            s.under_rated_restricted,
            percent(s.date),
            percent(s.tags_found),
            s.tags_outside,
            s.seconds_per_item,
            if short.is_empty() {
                "yes".to_owned()
            } else {
                format!("no: {}", short.join(", "))
            }
        )
        .unwrap();
    }
    text
}

/// A table cell's text: one line, with pipes escaped.
fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn report_markdown(measured: &[Measured], answers: &Answers) -> String {
    let mut text = format!(
        "# Benchmark report\n\n{} on {} {}, carrel {}.\n\n## Summary\n\n",
        jiff::Zoned::now().date(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        carrel_core::CARREL_VERSION
    );
    text.push_str(&summary_table(measured));
    for m in measured {
        let version = m
            .model
            .version
            .as_deref()
            .map(|v| format!(" (provider {v})"))
            .unwrap_or_default();
        write!(
            text,
            "\n## {}{version}\n\n| Item | Expected | Proposed | Sensitivity | Title | Summary |\n| --- | --- | --- | --- | --- | --- |\n",
            m.model.label
        )
        .unwrap();
        for (o, s) in &m.items {
            let expected = answers
                .items
                .get(&o.item)
                .map(|a| a.destinations.join(" or "))
                .unwrap_or_default();
            let mark = if s.routing >= 1.0 {
                "✓"
            } else if s.routing > 0.0 {
                "½"
            } else {
                "✗"
            };
            let level = match (s.sensitivity, o.sensitivity) {
                (Rating::Missing, _) | (_, None) => "none".to_owned(),
                (rating, Some(level)) => format!("{level:?} ({rating:?})").to_lowercase(),
            };
            writeln!(
                text,
                "| {} | {} | {mark} {} | {} | {} | {} |",
                cell(&o.item),
                cell(&expected),
                cell(o.destination.as_deref().unwrap_or("none")),
                level,
                cell(&o.title),
                cell(o.summary.as_deref().unwrap_or("")),
            )
            .unwrap();
        }
    }
    text
}
