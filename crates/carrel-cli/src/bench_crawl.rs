//! `carrel bench --crawl`: the crawler benchmark (`docs/design/restructure.md`, "Measuring
//! it"). It files the test set into a muddled copy of its archive (`muddle.yaml`), then asks
//! each text model to judge every item there as the crawler does, and writes `results.json` and
//! `report.md` to `crawl/` in the output folder. The scoring is in the core
//! ([`carrel_core::bench::crawl`]).
//!
//! Each judgment is saved to `crawl/runs/` as soon as it's given, so a stopped run carries on
//! where it stopped; `--fresh` starts over.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;
use std::time::Instant;

use carrel_core::archive::Archive;
use carrel_core::bench::crawl::{
    CRAWL_BAR, CrawlScore, ItemScore, Judged, Muddle, build, counts, score_item, summarize,
};
use carrel_core::bench::{Answers, check, percent};
use carrel_core::coordinator::Holder;
use carrel_core::crawl::Crawler;
use carrel_core::providers::Registry;
use carrel_core::providers::device::AvailableModel;
use serde_json::{Value, json};

use crate::bench::{BenchArgs, Model, cell, choose, file_safe};
use crate::{Failure, Report};

/// One model's judgments, scored.
struct Judging {
    model: Model,
    score: CrawlScore,
    items: Vec<(Judged, ItemScore)>,
}

/// `carrel bench --crawl`.
///
/// # Errors
///
/// [`Failure`] if the test set or its muddle are inconsistent, a model asked for isn't
/// available, or the archive or results can't be written.
pub fn crawl_bench(args: &BenchArgs, json: bool) -> Result<Report, Failure> {
    let corpus = args.corpus.as_path();
    let (answers, muddle) = load(corpus)?;
    let dir = args.out.join("crawl");
    if args.fresh {
        let _ = std::fs::remove_dir_all(&dir);
    }
    let archive = build(
        &corpus.join("archive"),
        &corpus.join("items"),
        &answers,
        &muddle,
        &carrel_core::bench::findability::Descriptions::load(&corpus.join("descriptions.yaml"))
            .map_err(Failure::new)?,
        &dir.join("archive"),
    )
    .map_err(Failure::new)?;
    let registry = crate::providers::registry().0;
    crate::search::read_texts(&archive, &registry, json)?;
    let models = choose(&registry, &args.models)?;
    let mut todo = to_judge(&archive, &answers)?;
    if let Some(limit) = args.limit {
        todo.truncate(limit);
    }
    let places = muddle.places(&answers);
    let existing: Vec<String> = archive.collections()?.into_iter().map(|c| c.path).collect();
    let runs = dir.join("runs");
    std::fs::create_dir_all(&runs).map_err(|e| Failure::io(&runs, &e))?;

    let mut judged = Vec::new();
    let count = models.len();
    for (n, model) in models.into_iter().enumerate() {
        let saved_path = runs.join(format!(
            "{}--{}.jsonl",
            file_safe(&model.provider),
            file_safe(&model.offer)
        ));
        let mut display =
            crate::progress::Display::new(format!("{} ({} of {count})", model.label, n + 1), json);
        let saved = judge_all(
            &archive,
            &registry,
            &model,
            &todo,
            &saved_path,
            &mut display,
        );
        display.finish();
        let saved = saved?;
        let items: Vec<(Judged, ItemScore)> = todo
            .iter()
            .filter_map(|(file, _)| {
                let gave = saved.get(file)?.clone();
                let (place, kind) = places.get(file)?;
                let score =
                    score_item(file, (place, *kind), &answers.items[file], &gave, &existing);
                Some((gave, score))
            })
            .collect();
        let scores: Vec<ItemScore> = items.iter().map(|(_, s)| s.clone()).collect();
        judged.push(Judging {
            model,
            score: summarize(&scores),
            items,
        });
    }

    let results = results_json(&judged);
    let results_path = dir.join("results.json");
    std::fs::write(&results_path, format!("{results:#}\n"))
        .map_err(|e| Failure::io(&results_path, &e))?;
    let report_path = dir.join("report.md");
    std::fs::write(&report_path, report_markdown(&judged))
        .map_err(|e| Failure::io(&report_path, &e))?;
    let mut text = summary_table(&judged);
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

/// The muddled archive's items that count, by file name, in file-name order, each with the
/// item as the archive has it.
fn to_judge(
    archive: &Archive,
    answers: &Answers,
) -> Result<Vec<(String, carrel_core::archive::ItemRef)>, Failure> {
    let mut items: Vec<(String, carrel_core::archive::ItemRef)> = archive
        .items()?
        .into_iter()
        .filter_map(|item| {
            let file = item.item.as_deref()?.rsplit('/').next()?.to_owned();
            answers
                .items
                .get(&file)
                .filter(|a| counts(a))
                .map(|_| (file, item))
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(items)
}

/// Has `model` judge every item in `todo` it hasn't judged in `saved_path` yet, saving each
/// judgment as it comes. Returns every judgment, by file name.
fn judge_all(
    archive: &Archive,
    registry: &Registry,
    model: &Model,
    todo: &[(String, carrel_core::archive::ItemRef)],
    saved_path: &Path,
    display: &mut crate::progress::Display,
) -> Result<BTreeMap<String, Judged>, Failure> {
    let mut saved = load_saved(saved_path);
    let left: Vec<&(String, carrel_core::archive::ItemRef)> = todo
        .iter()
        .filter(|(file, _)| !saved.contains_key(file))
        .collect();
    let total = todo.len();
    if left.is_empty() {
        return Ok(saved);
    }
    // Judging records nothing, so whose device it is doesn't matter.
    let crawler = Crawler {
        archive,
        registry,
        model: AvailableModel {
            provider: model.provider.clone(),
            offer: model.offer.clone(),
            label: model.label.clone(),
        },
        holder: Holder {
            person: carrel_core::card::Id::new_v7(),
            device: carrel_core::card::Id::new_v7(),
        },
    };
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(saved_path)
        .map_err(|e| Failure::io(saved_path, &e))?;
    for (done, (name, item)) in left.iter().enumerate() {
        display.count("items", total - left.len() + done, total, Some(name));
        let started = Instant::now();
        let result = crawler.judge_item(item);
        let seconds = started.elapsed().as_secs_f64();
        let gave = match result {
            Ok(Some(judgment)) => Judged {
                fits: Some(judgment.fits),
                better: judgment.better,
                confidence: judgment.confidence,
                reason: judgment.reason,
                seconds,
                error: None,
            },
            Ok(None) => unusable(seconds, "the model's answer wasn't usable".into()),
            Err(e) => unusable(seconds, e.to_string()),
        };
        // One line per judgment, written whole, so a stop mid-run loses at most this one.
        let line = json!({ "item": name, "judged": gave });
        writeln!(file, "{line}").map_err(|e| Failure::io(saved_path, &e))?;
        saved.insert(name.clone(), gave);
    }
    display.count("items", total, total, None);
    Ok(saved)
}

fn unusable(seconds: f64, error: String) -> Judged {
    Judged {
        fits: None,
        better: None,
        confidence: 0.0,
        reason: String::new(),
        seconds,
        error: Some(error),
    }
}

/// The judgments saved by an earlier run, skipping a line cut short when it was stopped.
fn load_saved(path: &Path) -> BTreeMap<String, Judged> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeMap::new();
    };
    text.lines()
        .filter_map(|line| {
            let value: Value = serde_json::from_str(line).ok()?;
            let item = value.get("item")?.as_str()?.to_owned();
            let judged = serde_json::from_value(value.get("judged")?.clone()).ok()?;
            Some((item, judged))
        })
        .collect()
}

fn results_json(judged: &[Judging]) -> Value {
    json!({
        "date": jiff::Zoned::now().date().to_string(),
        "carrel": carrel_core::CARREL_VERSION,
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "bar": CRAWL_BAR,
        "models": judged.iter().map(|j| {
            let short = j.score.shortfalls(&CRAWL_BAR);
            json!({
                "provider": j.model.provider,
                "offer": j.model.offer,
                "label": j.model.label,
                "version": j.model.version,
                "score": j.score,
                "meets_bar": short.is_empty(),
                "shortfalls": short,
                "items": j.items.iter().map(|(gave, score)| json!({
                    "judged": gave,
                    "score": score,
                })).collect::<Vec<_>>(),
            })
        }).collect::<Vec<_>>(),
    })
}

/// One row per model, and whether it meets the bar.
fn summary_table(judged: &[Judging]) -> String {
    let mut text = String::from(
        "| Model | Found | Recall | Precision | Placed right | Unusable | s/item | Meets the bar |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n",
    );
    for j in judged {
        let s = &j.score;
        let short = s.shortfalls(&CRAWL_BAR);
        writeln!(
            text,
            "| {} | {} of {} | {} | {} | {} | {} | {:.1} | {} |",
            j.model.label,
            s.found,
            s.misplaced,
            percent(s.recall),
            percent(s.precision),
            percent(s.placement),
            s.unusable,
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

/// The report: the summary, then for each model what it found by kind, what it missed, where
/// it put what it found, and its false alarms.
fn report_markdown(judged: &[Judging]) -> String {
    let items = judged.first().map_or(0, |j| j.score.items);
    let mut text = format!(
        "# Crawler benchmark report\n\n{} on {} {}, carrel {}, {items} items in the muddled archive.\n\n## Summary\n\n",
        jiff::Zoned::now().date(),
        std::env::consts::OS,
        std::env::consts::ARCH,
        carrel_core::CARREL_VERSION,
    );
    text.push_str(&summary_table(judged));
    writeln!(
        text,
        "\nThe bar: recall at least {}, precision at least {}, at least {} of what it finds put right, and under {:.0} seconds an item.",
        percent(CRAWL_BAR.recall),
        percent(CRAWL_BAR.precision),
        percent(CRAWL_BAR.placement),
        CRAWL_BAR.seconds_per_item
    )
    .unwrap();
    for j in judged {
        writeln!(text, "\n## {}\n", j.model.label).unwrap();
        for (kind, (found, of)) in &j.score.by_kind {
            writeln!(text, "- {}: found {found} of {of}", kind.describe()).unwrap();
        }
        let mut missed = Vec::new();
        let mut misplaced = Vec::new();
        let mut alarms = Vec::new();
        let mut failed = Vec::new();
        for (gave, score) in &j.items {
            let better = gave.better.as_deref().unwrap_or("nowhere better");
            if let Some(why) = &gave.error {
                failed.push(format!("{}: {why}", score.item));
            } else if score.misplaced.is_some() && !score.flagged {
                missed.push(format!("{} (in {})", score.item, score.place));
            } else if score.placed_right == Some(false) {
                misplaced.push(format!("{} → {better}", score.item));
            } else if score.misplaced.is_none() && score.flagged {
                alarms.push(format!(
                    "{} (in {}) → {better}: {}",
                    score.item, score.place, gave.reason
                ));
            }
        }
        for (title, list) in [
            ("Missed", missed),
            ("Found but put wrong", misplaced),
            ("False alarms", alarms),
            ("Unusable", failed),
        ] {
            if !list.is_empty() {
                writeln!(text, "\n{title}:\n").unwrap();
                for line in list {
                    writeln!(text, "- {}", cell(&line)).unwrap();
                }
            }
        }
    }
    text
}
