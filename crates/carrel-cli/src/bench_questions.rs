//! `carrel bench --questions`: question value (`docs/design/librarian.md`, "Measuring it").
//! With each text model, it ingests the test set in one batch into a copy of its archive,
//! answers each question the batch raises with the known answer, re-routes, and compares what
//! each answer won with the question queue's ranking. It writes `results.json` and `report.md`
//! to `questions/` in the output folder. The work is in the core
//! ([`carrel_core::bench::question_value`]).
//!
//! Each model's run is saved to `questions/runs/` when it's done, and reused by the next run;
//! `--fresh` starts over.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::bench::question_value::{QuestionValueRun, value_questions};
use carrel_core::bench::{Answers, check, percent};
use carrel_core::card::Id;
use carrel_core::providers::Capability;
use serde_json::{Value, json};

use crate::bench::{BenchArgs, Model, cell, choose, copy_dir, file_safe};
use crate::{Failure, Report};

/// `carrel bench --questions`.
///
/// # Errors
///
/// [`Failure`] if the test set is inconsistent, a model asked for isn't available, ingest
/// fails, or the results can't be written.
pub fn question_value_bench(args: &BenchArgs, json: bool) -> Result<Report, Failure> {
    let corpus = args.corpus.as_path();
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
    if let Some(limit) = args.limit {
        items.truncate(limit);
    }
    let dir = args.out.join("questions");
    let runs = dir.join("runs");
    if args.fresh {
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&runs).map_err(|e| Failure::io(&runs, &e))?;

    let (mut registry, _) = crate::providers::registry();
    registry.second_opinion(Capability::TextModel, None);
    let models = choose(&registry, &args.models)?;
    let mut measured = Vec::new();
    let count = models.len();
    for (n, model) in models.into_iter().enumerate() {
        let saved_path = runs.join(format!(
            "{}--{}.json",
            file_safe(&model.provider),
            file_safe(&model.offer)
        ));
        if let Some(saved) = load_saved(&saved_path) {
            measured.push((model, saved));
            continue;
        }
        let mut display =
            crate::progress::Display::new(format!("{} ({} of {count})", model.label, n + 1), json);
        display.count("batches", 0, 1, None);
        registry.pin(Capability::TextModel, &model.provider, &model.offer);
        let copy = std::env::temp_dir().join(format!("carrel-bench-questions-{}", Id::new_v7()));
        copy_dir(&fixture, &copy).map_err(|e| Failure::io(&copy, &e))?;
        let result = Archive::open(&copy)
            .map_err(Failure::from)
            .and_then(|archive| {
                value_questions(&archive, &registry, &items, &answers, Id::new_v7())
                    .map_err(Failure::new)
            });
        let _ = std::fs::remove_dir_all(&copy);
        display.count("batches", 1, 1, None);
        display.finish();
        let run = result?;
        let text = serde_json::to_string_pretty(&run).unwrap_or_default();
        std::fs::write(&saved_path, text + "\n").map_err(|e| Failure::io(&saved_path, &e))?;
        measured.push((model, run));
    }

    let results = results_json(&measured);
    let results_path = dir.join("results.json");
    std::fs::write(&results_path, format!("{results:#}\n"))
        .map_err(|e| Failure::io(&results_path, &e))?;
    let report_path = dir.join("report.md");
    std::fs::write(&report_path, report_markdown(&measured))
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

/// A model's run saved by an earlier `carrel bench --questions`, if there's one that reads.
fn load_saved(path: &Path) -> Option<QuestionValueRun> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn results_json(measured: &[(Model, QuestionValueRun)]) -> Value {
    json!({
        "date": jiff::Zoned::now().date().to_string(),
        "carrel": carrel_core::CARREL_VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "models": measured.iter().map(|(model, run)| json!({
            "provider": model.provider,
            "offer": model.offer,
            "label": model.label,
            "version": model.version,
            "value": run.value,
            "questions": run.valued,
            "proposals": run.proposals,
        })).collect::<Vec<_>>(),
    })
}

/// A share as a percentage, or a dash for none.
fn correlation(value: Option<f64>) -> String {
    value.map_or_else(|| "–".to_owned(), |c| format!("{c:.2}"))
}

/// The routing after answering the k highest-ranked, and the best k, for k = `k`.
fn top(run: &QuestionValueRun, k: usize) -> String {
    run.value.top_k.get(k - 1).map_or_else(
        || "–".to_owned(),
        |t| format!("{} ({})", percent(t.ranked), percent(t.best)),
    )
}

/// One row per model.
fn summary_table(measured: &[(Model, QuestionValueRun)]) -> String {
    let mut text = String::from(
        "| Model | Items | Questions | Answerable | Routing before | After all | Top 1 (best) | Top 3 (best) | Top 5 (best) | Rank correlation |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for (model, run) in measured {
        let v = &run.value;
        writeln!(
            text,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            model.label,
            v.items,
            v.questions,
            v.answerable,
            percent(v.routing_before),
            percent(v.routing_after),
            top(run, 1),
            top(run, 3),
            top(run, 5),
            correlation(v.correlation)
        )
        .unwrap();
    }
    text
}

/// The report: the summary, then each model's questions in the ranking's order.
fn report_markdown(measured: &[(Model, QuestionValueRun)]) -> String {
    let mut text = format!(
        "# Question value report\n\n{} on {} {}, carrel {}.\n\n## Summary\n\n",
        jiff::Zoned::now().date(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        carrel_core::CARREL_VERSION
    );
    text.push_str(&summary_table(measured));
    text.push_str(
        "\nTop k: the routing after answering the k highest-ranked questions, with the best any k questions give in brackets. Rank correlation: Spearman's, between what the ranking said each answer is worth and the routing points it won.\n",
    );
    for (model, run) in measured {
        writeln!(text, "\n## {}\n", model.label).unwrap();
        if run.value.set_aside > 0 {
            writeln!(
                text,
                "The ranking set {} questions aside as answered, merged or declined.\n",
                run.value.set_aside
            )
            .unwrap();
        }
        if run.valued.is_empty() {
            text.push_str("The batch raised no questions.\n");
            continue;
        }
        text.push_str("| Rank | Question | Items | Ranked value | Known answer | Moved | Gain |\n| --- | --- | --- | --- | --- | --- | --- |\n");
        for v in &run.valued {
            let moved: Vec<String> = v.moved.iter().map(|(i, d)| format!("{i} → {d}")).collect();
            writeln!(
                text,
                "| {} | {} | {} | {:.2} | {} | {} | {:+.1} |",
                v.rank,
                cell(&v.question),
                v.items,
                v.ranked_value,
                cell(
                    &v.answer
                        .as_ref()
                        .map_or_else(|| "none".to_owned(), |a| a.describe())
                ),
                cell(&moved.join("; ")),
                v.gain
            )
            .unwrap();
        }
    }
    text
}
