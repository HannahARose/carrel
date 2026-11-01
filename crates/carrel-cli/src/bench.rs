//! `carrel bench`: the model benchmark (`docs/design/benchmark.md`). It ingests the test set
//! with each model in turn, scores what each proposed, and writes `results.json` and a
//! side-by-side `report.md`. It's for people developing Carrel and choosing models, so it's
//! left out of `carrel --help`.
//!
//! Items are ingested one at a time, and each result is saved to `runs/` in the output folder
//! as soon as it's done. Stopping a run (Ctrl-C) loses at most the item in progress, and
//! running again with the same `--out` carries on where it stopped; `--fresh` starts over.
//!
//! `--search` runs the findability test instead ([`crate::findability`]), `--librarian` the
//! librarian's questions ([`crate::bench_librarian`]), and `--questions` question value
//! ([`crate::bench_questions`]).

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
#[derive(Clone)]
pub(crate) struct Model {
    pub(crate) provider: String,
    pub(crate) offer: String,
    pub(crate) label: String,
    pub(crate) version: Option<String>,
}

/// One model's results.
struct Measured {
    model: Model,
    score: Score,
    items: Vec<(Outcome, ItemScore)>,
}

/// `carrel bench`'s options.
#[derive(Debug, clap::Args)]
pub struct BenchArgs {
    /// A model to measure, as `provider/offer` or the offer alone; repeat for more.
    /// Defaults to every available text model. With `--search`, an embedding model by name,
    /// defaulting to every one on offer.
    #[arg(long = "model", value_name = "MODEL")]
    pub models: Vec<String>,
    /// The test set: a folder with `archive`, `items` and `answers.yaml`.
    #[arg(long, default_value = "fixtures/bench")]
    pub corpus: PathBuf,
    /// Where to write `results.json` and `report.md`.
    #[arg(long, default_value = "bench-results")]
    pub out: PathBuf,
    /// Use only the first N items, for a quick check.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
    /// Discard results saved in `--out` by an earlier run, instead of carrying on from them.
    /// Use it after changing prompts, checks or models.
    #[arg(long)]
    pub fresh: bool,
    /// Compare with an earlier run's `results.json`: writes `comparison.md`, and fails
    /// if a model that met the bar then no longer does.
    #[arg(long, value_name = "RESULTS")]
    pub compare: Option<PathBuf>,
    /// Ask this model for a second opinion on items each model isn't sure where to put, as
    /// ingest does with a recommended model. Without it, each model is measured alone.
    #[arg(long, value_name = "MODEL")]
    pub second: Option<String>,
    /// What to measure, when it isn't filing.
    #[command(flatten)]
    pub mode: BenchMode,
    /// With `--search`: title and summarize each item's card as a model drafted it in an
    /// earlier run's `results.json`, instead of by its file name; repeat for more.
    #[arg(long, value_name = "RESULTS", requires = "search", conflicts_with_all = ["librarian", "questions", "crawl", "restructure"])]
    pub cards: Vec<PathBuf>,
}

/// What `carrel bench` measures instead of filing: one at most.
#[derive(Debug, clap::Args)]
#[group(multiple = false)]
#[allow(clippy::struct_excessive_bools)] // one flag per mode, and clap allows only one
pub struct BenchMode {
    /// Measure search instead of filing: look up the test set's hand-written descriptions
    /// (`descriptions.yaml`) by words, by meaning and by both, with each embedding model, and
    /// write `results.json` and `report.md` to `search/` in `--out`. Each model's index is
    /// kept there and reused by the next run.
    #[arg(long, conflicts_with_all = ["compare", "second"])]
    pub search: bool,
    /// Measure the librarian instead of filing: ask each text model the test set's
    /// hand-written questions (`librarian.yaml`) over the filed test set, and write
    /// `results.json` and `report.md` to `librarian/` in `--out`.
    #[arg(long, conflicts_with_all = ["compare", "second"])]
    pub librarian: bool,
    /// Measure question value instead of filing: with each text model, ingest the test set in
    /// one batch, answer each question it raises with the known answer, and compare what each
    /// answer wins with the question queue's ranking. Writes `results.json` and `report.md` to
    /// `questions/` in `--out`.
    #[arg(long, conflicts_with_all = ["compare", "second"])]
    pub questions: bool,
    /// Measure the crawler instead of filing: file the test set into a muddled copy of its
    /// archive (`muddle.yaml`), have each text model judge every item there as the crawler
    /// does, and score how many misplaced items it finds and where it would put them. Writes
    /// `results.json` and `report.md` to `crawl/` in `--out`.
    #[arg(long, conflicts_with_all = ["compare", "second"])]
    pub crawl: bool,
    /// Measure restructure proposals instead of filing: on the same muddled archive, propose a
    /// restructure of the whole archive at each eagerness, with numbers alone and with each
    /// text model, and score whether it recovers the intended shape, its moves weighted by
    /// invasiveness, and its findability gain. Writes `results.json` and `report.md` to
    /// `restructure/` in `--out`.
    #[arg(long, conflicts_with_all = ["compare", "second", "limit"])]
    pub restructure: bool,
}

/// `carrel bench`.
///
/// # Errors
///
/// [`Failure`] if the test set is inconsistent, a model asked for isn't available, or the
/// results can't be written; or, with `--compare`, if a model no longer meets the bar.
pub fn bench(args: &BenchArgs, json: bool) -> Result<Report, Failure> {
    if args.mode.search {
        crate::findability::findability(args, json)
    } else if args.mode.librarian {
        crate::bench_librarian::librarian_bench(args, json)
    } else if args.mode.questions {
        crate::bench_questions::question_value_bench(args, json)
    } else if args.mode.crawl {
        crate::bench_crawl::crawl_bench(args, json)
    } else if args.mode.restructure {
        crate::bench_restructure::restructure_bench(args, json)
    } else {
        filing(args, json)
    }
}

/// The benchmark of filing: each text model ingests the test set.
fn filing(args: &BenchArgs, json: bool) -> Result<Report, Failure> {
    let (corpus, wanted, out, limit, fresh) = (
        args.corpus.as_path(),
        args.models.as_slice(),
        args.out.as_path(),
        args.limit,
        args.fresh,
    );
    let previous = load_previous(args.compare.as_deref())?;
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
    let second = set_second(&mut registry, args.second.as_deref())?;
    let mut models = choose(&registry, wanted)?;
    if let Some(second) = &second {
        for model in &mut models {
            write!(model.label, " then {}", second.label).unwrap();
        }
    }
    let mut measured = Vec::new();
    let count = models.len();
    for (n, model) in models.into_iter().enumerate() {
        let saved_path = runs.join(format!(
            "{}--{}{}.jsonl",
            file_safe(&model.provider),
            file_safe(&model.offer),
            second
                .as_ref()
                .map(|s| format!("--then-{}-{}", file_safe(&s.provider), file_safe(&s.offer)))
                .unwrap_or_default()
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
        measured.push(scored(model, saved, &answers));
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
    if let Some(previous) = &previous {
        compare_with(previous, &measured, out, &mut text)?;
    }
    Ok(Report::new(text, results))
}

/// Sets the model asked for a second opinion to `name`, or none, so each model is measured
/// alone unless `--second` says otherwise.
fn set_second(
    registry: &mut Registry,
    name: Option<&str>,
) -> Result<Option<crate::providers::TextModel>, Failure> {
    let second = name
        .map(|name| crate::providers::resolve_model(registry, name))
        .transpose()?;
    registry.second_opinion(
        Capability::TextModel,
        second
            .as_ref()
            .map(|m| (m.provider.as_str(), m.offer.as_str())),
    );
    Ok(second)
}

/// Writes `comparison.md` in `out` and adds it to `text`; fails, with `text` as the message,
/// if a model no longer meets the bar.
fn compare_with(
    previous: &Value,
    measured: &[Measured],
    out: &Path,
    text: &mut String,
) -> Result<(), Failure> {
    let (table, regressions) = comparison(previous, measured);
    let path = out.join("comparison.md");
    std::fs::write(&path, &table).map_err(|e| Failure::io(&path, &e))?;
    writeln!(text, "\n{table}").unwrap();
    if regressions.is_empty() {
        Ok(())
    } else {
        Err(Failure::new(format!(
            "{text}\nNo longer meets the bar: {}",
            regressions.join(", ")
        )))
    }
}

/// An earlier run's `results.json`, read before any work so a bad path fails at once.
fn load_previous(path: Option<&Path>) -> Result<Option<Value>, Failure> {
    path.map(|path| {
        let text = std::fs::read_to_string(path).map_err(|e| Failure::io(path, &e))?;
        serde_json::from_str(&text).map_err(|e| Failure::new(format!("{}: {e}", path.display())))
    })
    .transpose()
}

/// One model's results, from its saved outcomes and their times.
fn scored(model: Model, saved: Vec<(Outcome, f64)>, answers: &Answers) -> Measured {
    let seconds: f64 = saved.iter().map(|(_, s)| s).sum();
    let found: Vec<Outcome> = saved.into_iter().map(|(o, _)| o).collect();
    let (score, scores) = score(answers, &found, seconds);
    let mut items: Vec<(Outcome, ItemScore)> = Vec::new();
    for s in scores {
        if let Some(o) = found.iter().find(|o| o.item == s.item) {
            items.push((o.clone(), s));
        }
    }
    Measured {
        model,
        score,
        items,
    }
}

/// Each model's main scores beside the same model's in `previous` (an earlier
/// `results.json`), as Markdown, and the models that met the bar then and don't now.
fn comparison(previous: &Value, measured: &[Measured]) -> (String, Vec<String>) {
    let earlier = |m: &Measured| {
        previous["models"].as_array().and_then(|models| {
            models.iter().find(|p| {
                p["provider"].as_str() == Some(m.model.provider.as_str())
                    && p["offer"].as_str() == Some(m.model.offer.as_str())
            })
        })
    };
    let mut text = format!(
        "## Compared with the run of {} (Carrel {})\n\n\
         | Model | Valid | Routing | Under-rated | s/item | Meets the bar |\n\
         | --- | --- | --- | --- | --- | --- |\n",
        previous["date"].as_str().unwrap_or("an earlier date"),
        previous["carrel"].as_str().unwrap_or("?"),
    );
    let mut regressions = Vec::new();
    for m in measured {
        let before = earlier(m);
        let was = |key: &str| before.and_then(|p| p["score"][key].as_f64());
        let share = |now: f64, key: &str| match was(key) {
            Some(then) => format!("{} ({})", percent(now), points(now - then)),
            None => percent(now),
        };
        let seconds = match was("seconds_per_item") {
            Some(then) => format!(
                "{:.1} ({:+.1})",
                m.score.seconds_per_item,
                m.score.seconds_per_item - then
            ),
            None => format!("{:.1}", m.score.seconds_per_item),
        };
        let meets = m.score.shortfalls(&BAR).is_empty();
        let met = before.map(|p| p["shortfalls"].as_array().is_some_and(Vec::is_empty));
        let verdict = match (met, meets) {
            (Some(true), false) => {
                regressions.push(m.model.label.clone());
                "no, newly"
            }
            (Some(false), true) => "yes, newly",
            (_, true) => "yes",
            (_, false) => "no",
        };
        writeln!(
            text,
            "| {}{} | {} | {} | {} | {seconds} | {verdict} |",
            m.model.label,
            if before.is_none() { " (new)" } else { "" },
            share(m.score.valid, "valid"),
            share(m.score.routing, "routing"),
            share(m.score.under_rated, "under_rated"),
        )
        .unwrap();
    }
    (text, regressions)
}

/// A change in a share, in percentage points: `+3 pts`, `-12 pts`, `±0`.
fn points(change: f64) -> String {
    #[allow(clippy::cast_possible_truncation)] // a display value
    let rounded = (change * 100.0).round() as i64;
    if rounded == 0 {
        "±0".to_owned()
    } else {
        format!("{rounded:+} pts")
    }
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
pub(crate) fn file_safe(name: &str) -> String {
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
pub(crate) fn choose(registry: &Registry, wanted: &[String]) -> Result<Vec<Model>, Failure> {
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

pub(crate) fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
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
                    "what": a.what,
                })),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

/// The summary: one row per model, and whether it meets the bar.
fn summary_table(measured: &[Measured]) -> String {
    let mut text = String::from(
        "| Model | Valid | First try | Declined | Routing | Sensitivity exact | Under-rated | Restricted under | Dates | Tags found | Extra tags | Readings offered | Misread, right one offered | s/item | Meets the bar |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for m in measured {
        let s = &m.score;
        let short = s.shortfalls(&BAR);
        writeln!(
            text,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} of {} | {:.1} | {} |",
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
            percent(s.readings_offered),
            s.misread_offered,
            s.misread,
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
pub(crate) fn cell(text: &str) -> String {
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
            let mut level = match (s.sensitivity, o.sensitivity) {
                (Rating::Missing, _) | (_, None) => "none".to_owned(),
                (rating, Some(level)) => format!("{level:?} ({rating:?})").to_lowercase(),
            };
            if !o.reasons.is_empty() {
                write!(level, ": {}", o.reasons.join("; ")).unwrap();
            }
            writeln!(
                text,
                "| {} | {} | {mark} {} | {} | {} | {} |",
                cell(&o.item),
                cell(&expected),
                cell(o.destination.as_deref().unwrap_or("none")),
                cell(&level),
                cell(&o.title),
                cell(o.summary.as_deref().unwrap_or("")),
            )
            .unwrap();
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn measured(valid: f64) -> Measured {
        Measured {
            model: Model {
                provider: "llama".into(),
                offer: "small.gguf".into(),
                label: "llama/small".into(),
                version: None,
            },
            score: Score {
                items: 10,
                valid,
                first_try: valid,
                declined: 0.0,
                routing: 0.9,
                sensitivity_exact: 1.0,
                under_rated: 0.0,
                under_rated_restricted: 0,
                date: 1.0,
                tags_found: 1.0,
                tags_outside: 0,
                readings_offered: 0.0,
                misread: 0,
                misread_offered: 0,
                seconds_per_item: 10.0,
            },
            items: Vec::new(),
        }
    }

    fn previous(valid: f64, met: bool) -> Value {
        json!({
            "date": "2026-10-25",
            "carrel": "0.1.0",
            "models": [{
                "provider": "llama",
                "offer": "small.gguf",
                "score": {"valid": valid, "routing": 0.9, "under_rated": 0.0, "seconds_per_item": 12.0},
                "shortfalls": if met { json!([]) } else { json!(["valid output"]) },
            }],
        })
    }

    #[test]
    fn a_model_that_drops_below_the_bar_is_a_regression() {
        let (table, regressions) = comparison(&previous(1.0, true), &[measured(0.5)]);
        assert_eq!(regressions, ["llama/small"]);
        assert!(table.contains("50% (-50 pts)"), "{table}");
        assert!(table.contains("10.0 (-2.0)"), "{table}");
        assert!(table.contains("no, newly"), "{table}");

        let (table, regressions) = comparison(&previous(0.5, false), &[measured(1.0)]);
        assert_eq!(regressions, Vec::<String>::new());
        assert!(table.contains("yes, newly"), "{table}");

        let (table, regressions) = comparison(&json!({"models": []}), &[measured(0.5)]);
        assert_eq!(
            regressions,
            Vec::<String>::new(),
            "a new model can't regress"
        );
        assert!(table.contains("llama/small (new)"), "{table}");
    }

    #[test]
    fn changes_are_in_points() {
        assert_eq!(points(0.031), "+3 pts");
        assert_eq!(points(-0.12), "-12 pts");
        assert_eq!(points(0.001), "±0");
    }
}
