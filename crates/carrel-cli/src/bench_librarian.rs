//! `carrel bench --librarian`: the librarian benchmark (`docs/design/librarian.md`,
//! "Measuring it"). It files the test set into a copy of its archive, then asks each text model
//! the hand-written questions in `librarian.yaml`, and writes `results.json` and `report.md` to
//! `librarian/` in the output folder. The scoring is in the core
//! ([`carrel_core::bench::librarian`]).
//!
//! Each answer is saved to `librarian/runs/` as soon as it's given, so a stopped run carries
//! on where it stopped; `--fresh` starts over.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::bench::librarian::{
    AnswerScore, Gave, LIBRARIAN_BAR, LibrarianQuestions, LibrarianScore, RestrictedGuard, ask_one,
    check_librarian_questions, score_answer, summarize,
};
use carrel_core::bench::{Answers, check, findability, percent};
use carrel_core::librarian::Librarian;
use carrel_core::providers::Registry;
use carrel_core::providers::device::AvailableModel;
use serde_json::{Value, json};

use crate::bench::{BenchArgs, Model, cell, choose, file_safe};
use crate::{Failure, Report};

/// One model's answers, scored.
struct Asked {
    model: Model,
    score: LibrarianScore,
    answers: Vec<(Gave, AnswerScore)>,
}

/// `carrel bench --librarian`.
///
/// # Errors
///
/// [`Failure`] if the test set or its questions are inconsistent, a model asked for isn't
/// available, or the archive or results can't be written.
pub fn librarian_bench(args: &BenchArgs, json: bool) -> Result<Report, Failure> {
    let corpus = args.corpus.as_path();
    let (answers, mut questions) = load(corpus)?;
    if let Some(limit) = args.limit {
        questions.questions.truncate(limit);
    }
    let dir = args.out.join("librarian");
    if args.fresh {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let archive = findability::build(
        &corpus.join("archive"),
        &corpus.join("items"),
        &answers,
        &BTreeMap::new(),
        &dir.join("archive"),
    )
    .map_err(Failure::new)?;
    let registry = crate::providers::registry().0;
    crate::search::read_texts(&archive, &registry, json)?;
    let models = choose(&registry, &args.models)?;
    let guard = RestrictedGuard::new(&questions, &corpus.join("items"));
    let runs = dir.join("runs");
    std::fs::create_dir_all(&runs).map_err(|e| Failure::io(&runs, &e))?;

    let mut asked = Vec::new();
    let count = models.len();
    for (n, model) in models.into_iter().enumerate() {
        let saved_path = runs.join(format!(
            "{}--{}.jsonl",
            file_safe(&model.provider),
            file_safe(&model.offer)
        ));
        let mut display =
            crate::progress::Display::new(format!("{} ({} of {count})", model.label, n + 1), json);
        let saved = ask_all(
            &archive,
            &registry,
            &model,
            &questions,
            &saved_path,
            &mut display,
        );
        display.finish();
        let saved = saved?;
        let answers: Vec<(Gave, AnswerScore)> = questions
            .questions
            .iter()
            .filter_map(|q| {
                let gave = saved.get(&q.question)?.clone();
                let score = score_answer(q, &gave, &guard);
                Some((gave, score))
            })
            .collect();
        let scores: Vec<AnswerScore> = answers.iter().map(|(_, s)| s.clone()).collect();
        asked.push(Asked {
            model,
            score: summarize(&scores),
            answers,
        });
    }

    let results = results_json(&asked, &questions);
    let results_path = dir.join("results.json");
    std::fs::write(&results_path, format!("{results:#}\n"))
        .map_err(|e| Failure::io(&results_path, &e))?;
    let report_path = dir.join("report.md");
    std::fs::write(&report_path, report_markdown(&asked, &questions))
        .map_err(|e| Failure::io(&report_path, &e))?;
    let mut text = summary_table(&asked);
    writeln!(
        text,
        "\nWrote {} and {}.",
        results_path.display(),
        report_path.display()
    )
    .unwrap();
    Ok(Report::new(text, results))
}

/// The answers and questions in `corpus`, once both are known to agree with the test set.
fn load(corpus: &Path) -> Result<(Answers, LibrarianQuestions), Failure> {
    let answers = Answers::load(&corpus.join("answers.yaml")).map_err(Failure::new)?;
    let questions =
        LibrarianQuestions::load(&corpus.join("librarian.yaml")).map_err(Failure::new)?;
    let mut problems = check(
        &answers,
        &Archive::open(&corpus.join("archive"))?,
        &corpus.join("items"),
    )
    .map_err(Failure::new)?;
    problems.extend(check_librarian_questions(&questions, &answers));
    if problems.is_empty() {
        Ok((answers, questions))
    } else {
        Err(Failure::new(format!(
            "the test set is inconsistent:\n  {}",
            problems.join("\n  ")
        )))
    }
}

/// Asks `model` every question it hasn't answered in `saved_path` yet, saving each answer as
/// it comes. Returns every answer, by question.
fn ask_all(
    archive: &Archive,
    registry: &Registry,
    model: &Model,
    questions: &LibrarianQuestions,
    saved_path: &Path,
    display: &mut crate::progress::Display,
) -> Result<BTreeMap<String, Gave>, Failure> {
    let mut saved = load_saved(saved_path);
    let todo: Vec<&str> = questions
        .questions
        .iter()
        .map(|q| q.question.as_str())
        .filter(|q| !saved.contains_key(*q))
        .collect();
    let total = questions.questions.len();
    if todo.is_empty() {
        return Ok(saved);
    }
    let providers = || crate::providers::registry().0;
    let librarian = Librarian::new(
        archive,
        registry,
        AvailableModel {
            provider: model.provider.clone(),
            offer: model.offer.clone(),
            label: model.label.clone(),
        },
        &providers,
    )?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(saved_path)
        .map_err(|e| Failure::io(saved_path, &e))?;
    for (done, question) in todo.iter().enumerate() {
        display.count(
            "questions",
            total - todo.len() + done,
            total,
            Some(question),
        );
        let gave = ask_one(&librarian, question)?;
        // One line per answer, written whole, so a stop mid-run loses at most this one.
        let line = json!({ "question": question, "gave": gave });
        writeln!(file, "{line}").map_err(|e| Failure::io(saved_path, &e))?;
        saved.insert((*question).to_owned(), gave);
    }
    display.count("questions", total, total, None);
    Ok(saved)
}

/// The answers saved by an earlier run, skipping a line cut short when it was stopped.
fn load_saved(path: &Path) -> BTreeMap<String, Gave> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    text.lines()
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            let question = value.get("question")?.as_str()?.to_owned();
            let gave = serde_json::from_value(value.get("gave")?.clone()).ok()?;
            Some((question, gave))
        })
        .collect()
}

fn results_json(asked: &[Asked], questions: &LibrarianQuestions) -> Value {
    json!({
        "date": jiff::Zoned::now().date().to_string(),
        "carrel": carrel_core::CARREL_VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "questions": questions.questions.len(),
        "bar": {
            "recall": LIBRARIAN_BAR.recall,
            "precision": LIBRARIAN_BAR.precision,
            "facts": LIBRARIAN_BAR.facts,
            "leaks": LIBRARIAN_BAR.leaks,
            "seconds_per_question": LIBRARIAN_BAR.seconds_per_question,
        },
        "models": asked.iter().map(|a| {
            let short = a.score.shortfalls(&LIBRARIAN_BAR);
            json!({
                "provider": a.model.provider,
                "offer": a.model.offer,
                "label": a.model.label,
                "version": a.model.version,
                "score": a.score,
                "meets_bar": short.is_empty(),
                "shortfalls": short,
                "answers": a.answers.iter().map(|(gave, score)| json!({
                    "gave": gave,
                    "score": score,
                })).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
    })
}

/// One row per model, and whether it meets the bar.
fn summary_table(asked: &[Asked]) -> String {
    let mut text = String::from(
        "| Model | Recall | Precision | Facts | Restricted shown | Unanswered | Steps | Tokens | s/question | Meets the bar |\n| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for a in asked {
        let s = &a.score;
        let short = s.shortfalls(&LIBRARIAN_BAR);
        writeln!(
            text,
            "| {} | {} | {} | {} | {} | {} | {:.1} | {:.0} | {:.1} | {} |",
            a.model.label,
            percent(s.recall),
            percent(s.precision),
            percent(s.facts),
            s.leaks,
            s.unanswered,
            s.steps_per_question,
            s.tokens_per_question,
            s.seconds_per_question,
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

/// The report: the summary, then each model's answers beside what was expected.
fn report_markdown(asked: &[Asked], questions: &LibrarianQuestions) -> String {
    let mut text = format!(
        "# Librarian benchmark report\n\n{} on {} {}, carrel {}, {} questions.\n\n## Summary\n\n",
        jiff::Zoned::now().date(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        carrel_core::CARREL_VERSION,
        questions.questions.len()
    );
    text.push_str(&summary_table(asked));
    writeln!(
        text,
        "\nThe bar: recall at least {}, precision at least {}, facts at least {}, no restricted text shown, and under {:.0} seconds a question.",
        percent(LIBRARIAN_BAR.recall),
        percent(LIBRARIAN_BAR.precision),
        percent(LIBRARIAN_BAR.facts),
        LIBRARIAN_BAR.seconds_per_question
    )
    .unwrap();
    for a in asked {
        writeln!(text, "\n## {}\n", a.model.label).unwrap();
        for (gave, score) in &a.answers {
            write_answer(&mut text, gave, score);
        }
    }
    text
}

/// One answer, with what it missed.
fn write_answer(text: &mut String, gave: &Gave, score: &AnswerScore) {
    writeln!(
        text,
        "### {}\n\nItems: {} of {} right, {} came back. Facts: {} of {}. Steps: {}. {:.1} s.\n",
        score.question,
        score.right,
        score.expected,
        score.returned,
        score.facts_stated,
        score.facts_expected,
        score.steps,
        score.seconds
    )
    .unwrap();
    let mut notes = Vec::new();
    if !score.leaks.is_empty() {
        notes.push(format!(
            "**Restricted text shown:** {}",
            score.leaks.join("; ")
        ));
    }
    if !score.missed.is_empty() {
        notes.push(format!("Missed: {}", score.missed.join(", ")));
    }
    if !score.extra.is_empty() {
        notes.push(format!("Also came back: {}", score.extra.join(", ")));
    }
    if !score.facts_missing.is_empty() {
        notes.push(format!("Didn't say: {}", score.facts_missing.join("; ")));
    }
    if let Some(why) = &gave.failed {
        notes.push(format!("Failed: {why}"));
    }
    if let Some(stopped) = &gave.stopped {
        notes.push(format!("Stopped: out of {stopped}"));
    }
    if !gave.steps.is_empty() {
        notes.push(format!("Tools: {}", gave.steps.join("; ")));
    }
    for note in notes {
        writeln!(text, "- {}", cell(&note)).unwrap();
    }
    if !gave.text.trim().is_empty() {
        writeln!(text, "\n> {}", gave.text.trim().replace('\n', "\n> ")).unwrap();
    }
    text.push('\n');
}
