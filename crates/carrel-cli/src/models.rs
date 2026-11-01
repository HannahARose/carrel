//! `carrel models`: the models Carrel can download for local providers, and downloading or
//! removing one (`docs/design/models.md`). Nothing is downloaded without being asked for.

use std::fmt::Write as _;
use std::io::{BufRead as _, IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};

use carrel_core::archive::Archive;
use carrel_core::models::{
    CATALOG, Fetcher, Helper, Installed, Model, download as fetch_model, download_mlx,
    embedding_models, find, installed, installed_mlx, mlx_dir, network_log, part_path,
    remove as remove_model, remove_mlx, text_models,
};
use carrel_core::providers::{EmbeddingRequest, ProviderError, Purpose};
use carrel_core::settings::Key;
use serde_json::{Value, json};

use crate::ingest::human_size;
use crate::{Failure, Report};

/// A model's file format: one GGUF file for the llama.cpp provider, or a folder for MLX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Format {
    /// One GGUF file, for the llama.cpp provider (any platform).
    #[default]
    Gguf,
    /// A folder for the MLX provider (Apple-silicon Macs).
    Mlx,
}

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

/// A catalog model that can be downloaded: one whose file is pinned to a checksum.
fn downloadable(name: &str) -> Result<&'static Model, Failure> {
    let model = known(name)?;
    if let Some(provider) = model.comes_with {
        return Err(Failure::new(
            carrel_core::models::ModelError::ComesWith(model.name.to_owned(), provider).to_string(),
        ));
    }
    if model.is_pinned() {
        Ok(model)
    } else {
        Err(Failure::new(
            carrel_core::models::ModelError::Unpinned(model.name.to_owned()).to_string(),
        ))
    }
}

/// A catalog model's state in the models folder.
fn state(model: &Model, dir: &Path, found: &[Installed]) -> (&'static str, Option<u64>) {
    if model.comes_with.is_some() {
        return ("built in", None);
    }
    if found.iter().any(|f| f.name == model.name) {
        return ("downloaded", None);
    }
    match std::fs::metadata(part_path(model, dir)) {
        Ok(m) => ("partial", Some(m.len())),
        Err(_) => ("available", None),
    }
}

/// What `carrel models` adds after a model: recommended, downloaded, or how much of it.
fn state_note(model: &Model, state: &str, part: Option<u64>) -> String {
    let shown = match part {
        Some(bytes) => format!(
            "{} of {} downloaded",
            human_size(bytes),
            human_size(model.size)
        ),
        None if state == "downloaded" => "downloaded".to_owned(),
        None if state == "built in" => "nothing to download".to_owned(),
        None if !model.is_pinned() => "not yet pinned to a checksum".to_owned(),
        None => String::new(),
    };
    if !model.recommended {
        return shown;
    }
    let note = format!("recommended for {} GB", model.memory_gb);
    if shown.is_empty() {
        note
    } else {
        format!("{note}, {shown}")
    }
}

/// One catalog model's line in `carrel models`, with its MLX build's, and its JSON entry.
fn row(
    text: &mut String,
    model: &Model,
    dir: &Path,
    found: (&[Installed], &[Installed]),
    width: usize,
) -> Value {
    let (state, part) = state(model, dir, found.0);
    let shown = state_note(model, state, part);
    let size = if model.is_pinned() {
        human_size(model.size)
    } else if model.comes_with.is_some() {
        "-".to_owned()
    } else {
        "?".to_owned()
    };
    writeln!(
        text,
        "  {:width$}  {size:>6}  needs {:>2} GB memory  {}  {}{}",
        model.name,
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
    let mlx_state = model.mlx.map(|build| {
        if found.1.iter().any(|f| f.name == model.name) {
            writeln!(text, "  {:width$}  MLX build downloaded", "").unwrap();
            "downloaded"
        } else if mlx_dir(dir).join(format!("{}.part", model.name)).is_dir() {
            writeln!(text, "  {:width$}  MLX build partly downloaded", "").unwrap();
            "partial"
        } else {
            writeln!(
                text,
                "  {:width$}  also as MLX, {}: --format mlx",
                "",
                human_size(build.size())
            )
            .unwrap();
            "available"
        }
    });
    let mut entry = serde_json::to_value(model).unwrap_or(Value::Null);
    entry["state"] = json!(state);
    entry["downloaded_bytes"] = json!(part);
    entry["mlx_state"] = json!(mlx_state);
    entry["pinned"] = json!(model.is_pinned());
    entry
}

/// `carrel models`: the catalog, what's downloaded, and other models in the folder.
///
/// # Errors
///
/// [`Failure`] if there's no models folder or it can't be read.
pub fn list() -> Result<Report, Failure> {
    let dir = folder()?;
    let found = installed(&dir).map_err(|e| Failure::io(&dir, &e))?;
    let found_mlx = installed_mlx(&dir).map_err(|e| Failure::io(&dir, &e))?;
    let mut text = format!(
        "Models for the llama.cpp and MLX providers, kept in {}\n\n",
        dir.display()
    );
    let width = CATALOG.iter().map(|m| m.name.len()).max().unwrap_or(0);
    let mut catalog = Vec::new();
    for model in text_models() {
        catalog.push(row(&mut text, model, &dir, (&found, &found_mlx), width));
    }
    text.push_str("\nFor search by meaning, one per archive, chosen when it's created:\n");
    for model in embedding_models() {
        catalog.push(row(&mut text, model, &dir, (&found, &found_mlx), width));
    }
    let others: Vec<&Installed> = found
        .iter()
        .chain(&found_mlx)
        .filter(|f| find(&f.name).is_none())
        .collect();
    if !others.is_empty() {
        text.push_str("\nAlso in the folder:\n");
        for other in &others {
            writeln!(text, "  {}  {}", other.name, human_size(other.size)).unwrap();
        }
    }
    let recommended = CATALOG.iter().any(|m| m.recommended);
    if recommended {
        text.push_str(
            "\nRecommended models met the benchmark's bar; the results are in docs/bench/.\n",
        );
    } else {
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
pub(crate) fn confirm(question: &str, yes: bool) -> Result<bool, Failure> {
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

/// `carrel models download NAME [--format mlx]`: shows the size, license and destination,
/// asks, then downloads through the network helper.
///
/// # Errors
///
/// [`Failure`] if the model isn't known or has no build in that format, the helper is
/// missing, or the download fails.
pub fn download(name: &str, format: Format, yes: bool, json: bool) -> Result<Report, Failure> {
    let model = downloadable(name)?;
    let dir = folder()?;
    let (destination, size, provider) = match format {
        Format::Gguf => (dir.join(model.file_name()), model.size, "llama.cpp"),
        Format::Mlx => {
            let build = model.mlx.ok_or_else(|| {
                Failure::new(format!(
                    "{name} has no MLX build; download it as GGUF instead"
                ))
            })?;
            (mlx_dir(&dir).join(model.name), build.size(), "MLX")
        }
    };
    if destination.exists() {
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
        "Download {}{} ({}, {} license) from huggingface.co to {}?",
        model.title,
        if format == Format::Mlx {
            ", for MLX"
        } else {
            ""
        },
        human_size(size),
        model.license,
        destination.display()
    );
    if !confirm(&question, yes)? {
        return Err(Failure::new("nothing downloaded"));
    }
    let path = run(model, format, &dir, &helper, json)?;
    Ok(Report::new(
        format!(
            "Downloaded {} to {}; the {provider} provider offers it now.\n",
            model.name,
            path.display()
        ),
        json!({ "name": model.name, "path": path, "downloaded": true }),
    ))
}

/// Downloads with a progress line on a terminal, or progress as JSON lines on standard error
/// with `--json`.
fn run(
    model: &Model,
    format: Format,
    dir: &Path,
    fetcher: &dyn Fetcher,
    json: bool,
) -> Result<PathBuf, Failure> {
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
    let result = match format {
        Format::Gguf => fetch_model(model, dir, fetcher, log.as_deref(), &mut progress),
        Format::Mlx => download_mlx(model, dir, fetcher, log.as_deref(), &mut progress),
    };
    if terminal && !json {
        eprintln!();
    }
    result.map_err(|e| {
        let kept = match format {
            Format::Gguf => part_path(model, dir),
            Format::Mlx => mlx_dir(dir).join(format!("{}.part", model.name)),
        };
        if kept.exists() {
            Failure::new(format!(
                "{e}; the partial download is kept, and running this again continues it"
            ))
        } else {
            Failure::new(e.to_string())
        }
    })
}

/// Downloads a catalog model without asking and without drawing progress, for the interactive
/// mode, which has already asked and has the terminal.
///
/// # Errors
///
/// As [`download`].
pub fn download_quietly(name: &str, format: Format) -> Result<Report, Failure> {
    let model = downloadable(name)?;
    let dir = folder()?;
    let helper = Helper::find()?;
    let log = network_log();
    let path = match format {
        Format::Gguf => fetch_model(model, &dir, &helper, log.as_deref(), &mut |_, _| {})?,
        Format::Mlx => download_mlx(model, &dir, &helper, log.as_deref(), &mut |_, _| {})?,
    };
    Ok(Report::new(
        format!("Downloaded {} to {}\n", model.name, path.display()),
        json!({ "name": model.name, "path": path }),
    ))
}

/// `carrel models remove NAME [--format mlx]`: deletes a downloaded model, after asking.
///
/// # Errors
///
/// [`Failure`] if it isn't downloaded or can't be deleted.
pub fn remove(name: &str, format: Format, yes: bool) -> Result<Report, Failure> {
    let dir = folder()?;
    let shown = match format {
        Format::Gguf => format!("{name} from {}", dir.display()),
        Format::Mlx => format!("the MLX build of {name} from {}", mlx_dir(&dir).display()),
    };
    if !confirm(&format!("Delete {shown}?"), yes)? {
        return Err(Failure::new("nothing deleted"));
    }
    let path = match format {
        Format::Gguf => remove_model(name, &dir)?,
        Format::Mlx => remove_mlx(name, &dir)?,
    };
    Ok(Report::new(
        format!("Deleted {}.\n", path.display()),
        json!({ "name": name, "path": path }),
    ))
}

/// The archive's embedding model, asking which to use on first use in an archive that has
/// none and recording the answer in its settings, which every device follows.
///
/// # Errors
///
/// [`Failure`] if there's none and no terminal to ask in, or recording it fails.
pub fn archive_model(archive: &Archive) -> Result<String, Failure> {
    if let Some(model) = archive.settings().embedding_model() {
        return Ok(model.to_owned());
    }
    let choices = Key::EmbeddingModel.choices().unwrap_or_default();
    if !std::io::stdin().is_terminal() {
        return Err(Failure::new(format!(
            "this archive has no embedding model yet; choose one with `carrel settings set embedding.model NAME`: {}",
            choices.join(", ")
        )));
    }
    eprintln!(
        "This archive has no embedding model yet. Every device that searches it will use the one chosen here:"
    );
    for (n, name) in choices.iter().enumerate() {
        let title = find(name).map_or("", |m| m.title);
        let default = if n == 0 { " (the default)" } else { "" };
        eprintln!("  {}. {name}  {title}{default}", n + 1);
    }
    eprint!("Which? [1] ");
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    let answer = answer.trim();
    let chosen = if answer.is_empty() {
        choices[0]
    } else {
        answer
            .parse::<usize>()
            .ok()
            .and_then(|n| choices.get(n.wrapping_sub(1)).copied())
            .or_else(|| choices.iter().copied().find(|c| *c == answer))
            .ok_or_else(|| Failure::new(format!("{answer} isn't one of the choices")))?
    };
    let report = crate::settings::set(Some(archive.root()), Key::EmbeddingModel.as_str(), chosen)?;
    eprint!("{}", report.text);
    Ok(chosen.to_owned())
}

/// `carrel models embed TEXT... [--query]`: embeds texts with the archive's embedding model,
/// as search will, and shows the vectors. The model is asked for if the archive has none.
///
/// # Errors
///
/// [`Failure`] if the archive can't be found, it has no model and none can be asked for, or
/// no provider on this device runs its model.
pub fn embed(archive: Option<&Path>, texts: &[String], query: bool) -> Result<Report, Failure> {
    let archive = crate::commands::locate(archive)?;
    let model = archive_model(&archive)?;
    let (registry, _) = crate::providers::registry();
    let request = EmbeddingRequest {
        texts: texts.to_vec(),
        purpose: if query {
            Purpose::Query
        } else {
            Purpose::Document
        },
    };
    let (embeddings, source, _) = registry.embed(&model, &request).map_err(|e| match e {
        ProviderError::Unavailable(_) => match find(&model) {
            Some(Model {
                comes_with: Some(provider),
                ..
            }) => Failure::new(format!(
                "{e}; it comes with {provider}, which only Apple devices have"
            )),
            Some(_) => Failure::new(format!(
                "{e}; download it with `carrel models download {model}`"
            )),
            None => Failure::new(e.to_string()),
        },
        _ => Failure::new(e.to_string()),
    })?;
    let mut text = format!(
        "{} with {model} through {}, {} numbers each:\n",
        if texts.len() == 1 {
            "1 text embedded".to_owned()
        } else {
            format!("{} texts embedded", texts.len())
        },
        source.provider,
        embeddings.dimensions
    );
    for (t, vector) in texts.iter().zip(&embeddings.vectors) {
        let start: Vec<String> = vector.iter().take(4).map(|n| format!("{n:.4}")).collect();
        writeln!(text, "  [{}, …]  {t}", start.join(", ")).unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "model": embeddings.model,
            "dimensions": embeddings.dimensions,
            "provider": source.provider,
            "offer": source.offer,
            "purpose": request.purpose,
            "vectors": embeddings.vectors,
        }),
    ))
}
