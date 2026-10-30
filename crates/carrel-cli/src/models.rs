//! `carrel models`: the models Carrel can download for local providers, and downloading or
//! removing one (`docs/design/models.md`). Nothing is downloaded without being asked for.

use std::fmt::Write as _;
use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};

use carrel_core::models::{
    CATALOG, Fetcher, Helper, Installed, Model, download as fetch_model, find, installed,
    network_log, part_path, remove as remove_model,
};
use serde_json::{Value, json};

use crate::ingest::human_size;
use crate::{Failure, Report};

fn folder() -> Result<PathBuf, Failure> {
    carrel_core::local::models_dir()
        .ok_or_else(|| Failure::new("no models folder: set CARREL_MODELS_DIR"))
}

fn known(name: &str) -> Result<&'static Model, Failure> {
    find(name).ok_or_else(|| {
        Failure::new(format!(
            "no model called {name}; `carrel models` lists them"
        ))
    })
}

/// A catalog model's state in the models folder.
fn state(model: &Model, dir: &Path, found: &[Installed]) -> (&'static str, Option<u64>) {
    if found.iter().any(|f| f.name == model.name) {
        return ("downloaded", None);
    }
    match std::fs::metadata(part_path(model, dir)) {
        Ok(m) => ("partial", Some(m.len())),
        Err(_) => ("available", None),
    }
}

/// `carrel models`: the catalog, what's downloaded, and other models in the folder.
///
/// # Errors
///
/// [`Failure`] if there's no models folder or it can't be read.
pub fn list() -> Result<Report, Failure> {
    let dir = folder()?;
    let found = installed(&dir).map_err(|e| Failure::io(&dir, &e))?;
    let mut text = format!(
        "Models for the llama.cpp provider, kept in {}\n\n",
        dir.display()
    );
    let width = CATALOG.iter().map(|m| m.name.len()).max().unwrap_or(0);
    let mut catalog = Vec::new();
    for model in CATALOG {
        let (state, part) = state(model, &dir, &found);
        let shown = match part {
            Some(bytes) => format!(
                "{} of {} downloaded",
                human_size(bytes),
                human_size(model.size)
            ),
            None if state == "downloaded" => "downloaded".to_owned(),
            None => String::new(),
        };
        writeln!(
            text,
            "  {:width$}  {:>6}  needs {:>2} GB memory  {}  {}{}",
            model.name,
            human_size(model.size),
            model.memory_gb,
            model.license,
            model.title,
            if shown.is_empty() {
                String::new()
            } else {
                format!("  ({shown})")
            },
        )
        .unwrap();
        let mut entry = serde_json::to_value(model).unwrap_or(Value::Null);
        entry["state"] = json!(state);
        entry["downloaded_bytes"] = json!(part);
        catalog.push(entry);
    }
    let others: Vec<&Installed> = found.iter().filter(|f| find(&f.name).is_none()).collect();
    if !others.is_empty() {
        text.push_str("\nAlso in the folder:\n");
        for other in &others {
            writeln!(text, "  {}  {}", other.name, human_size(other.size)).unwrap();
        }
    }
    let recommended = CATALOG.iter().any(|m| m.recommended);
    if !recommended {
        text.push_str(
            "\nThis list is provisional: none is recommended until the model benchmark chooses.\n",
        );
    }
    text.push_str("Download one with `carrel models download NAME`.\n");
    let log = network_log();
    if let Some(log) = &log {
        writeln!(text, "Every download is logged in {}.", log.display()).unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "folder": dir,
            "catalog": catalog,
            "others": others,
            "provisional": !recommended,
            "log": log,
        }),
    ))
}

/// Asks `question` on the terminal, unless `yes` already answered it.
fn confirm(question: &str, yes: bool) -> Result<bool, Failure> {
    if yes {
        return Ok(true);
    }
    if !std::io::stdin().is_terminal() {
        return Err(Failure::new(
            "not in a terminal to ask; add --yes to go ahead without asking",
        ));
    }
    eprint!("{question} [y/N] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

/// `carrel models download NAME`: shows the size, license and destination, asks, then
/// downloads through the network helper.
///
/// # Errors
///
/// [`Failure`] if the model isn't known, the helper is missing, or the download fails.
pub fn download(name: &str, yes: bool, json: bool) -> Result<Report, Failure> {
    let model = known(name)?;
    let dir = folder()?;
    let destination = dir.join(model.file_name());
    if destination.is_file() {
        return Ok(Report::new(
            format!(
                "{} is already downloaded, at {}.\n",
                model.name,
                destination.display()
            ),
            json!({ "name": model.name, "path": destination, "downloaded": false }),
        ));
    }
    let helper = Helper::find()?;
    let question = format!(
        "Download {} ({}, {} license) from huggingface.co to {}?",
        model.title,
        human_size(model.size),
        model.license,
        destination.display()
    );
    if !confirm(&question, yes)? {
        return Err(Failure::new("nothing downloaded"));
    }
    let path = run(model, &dir, &helper, json)?;
    Ok(Report::new(
        format!(
            "Downloaded {} to {}; the llama.cpp provider offers it now.\n",
            model.name,
            path.display()
        ),
        json!({ "name": model.name, "path": path, "downloaded": true }),
    ))
}

/// Downloads with a progress line on a terminal, or progress as JSON lines on standard error
/// with `--json`.
fn run(model: &Model, dir: &Path, fetcher: &dyn Fetcher, json: bool) -> Result<PathBuf, Failure> {
    let terminal = std::io::stderr().is_terminal();
    let log = network_log();
    let mut progress = |done: u64, total: u64| {
        if json {
            eprintln!(
                "{}",
                json!({ "event": "progress", "done": done, "total": total })
            );
        } else if terminal {
            let percent = done.saturating_mul(100).checked_div(total).unwrap_or(100);
            eprint!(
                "\r{}: {percent:>3}% ({} of {})   ",
                model.name,
                human_size(done),
                human_size(total)
            );
            let _ = std::io::stderr().flush();
        }
    };
    let result = fetch_model(model, dir, fetcher, log.as_deref(), &mut progress);
    if terminal && !json {
        eprintln!();
    }
    result.map_err(|e| {
        let kept = part_path(model, dir);
        if kept.is_file() {
            Failure::new(format!(
                "{e}; the partial download is kept, and running this again continues it"
            ))
        } else {
            Failure::new(e.to_string())
        }
    })
}

/// `carrel models remove NAME`: deletes a downloaded model, after asking.
///
/// # Errors
///
/// [`Failure`] if it isn't downloaded or can't be deleted.
pub fn remove(name: &str, yes: bool) -> Result<Report, Failure> {
    let dir = folder()?;
    if !confirm(&format!("Delete {name} from {}?", dir.display()), yes)? {
        return Err(Failure::new("nothing deleted"));
    }
    let path = remove_model(name, &dir)?;
    Ok(Report::new(
        format!("Deleted {}.\n", path.display()),
        json!({ "name": name, "path": path }),
    ))
}
