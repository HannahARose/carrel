//! `carrel index`: the embedding index search finds items by meaning with
//! (`docs/design/search.md`): how far behind it is, rebuilding it, and switching models.
//!
//! The worker keeps it current in the background after every change; these commands report on
//! it, rebuild it by hand, and switch the archive to another model, building the new model's
//! index while the old one keeps serving. Only the archive's embedding model is used, never
//! another.

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock, Holder};
use carrel_core::index::switch::{self, Switch};
use carrel_core::index::{Index, IndexError, embed_query, plan};
use carrel_core::models::embedding_models;
use carrel_core::providers::{Capability, Registry};
use serde_json::{Value, json};

use crate::commands::{holder, locate, plural};
use crate::config::Identity;
use crate::{Failure, Report};

pub use carrel_core::search::run::how_to_get;

/// `error`, in words, with how to get `model` when no provider here has it.
#[allow(clippy::needless_pass_by_value)] // `map_err` hands the error over.
pub fn failure(error: IndexError, model: &str) -> Failure {
    Failure::new(carrel_core::search::run::index_failure(&error, model))
}

/// The embedding models providers on this device offer, best first, each once, with the
/// provider that runs it and how many numbers its vectors have, if it says.
pub fn offered_models(registry: &Registry) -> Vec<(String, String, Option<u64>)> {
    let mut models: Vec<(String, String, Option<u64>)> = Vec::new();
    for (provider, offer) in registry.ranked(Capability::Embed) {
        if !models.iter().any(|(id, _, _)| *id == offer.id) {
            let dimensions = offer.extra.get("dimensions").and_then(Value::as_u64);
            models.push((offer.id.clone(), provider.name().to_owned(), dimensions));
        }
    }
    models
}

/// Whether a provider on this device offers the embedding model `model`.
pub fn offered(registry: &Registry, model: &str) -> bool {
    carrel_core::worker::offered(registry, model)
}

/// `carrel index status`: the archive's embedding model, how many items are embedded, and how
/// many are behind.
///
/// # Errors
///
/// [`Failure`] if there's no archive or it can't be read.
pub fn status(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let mut switching = String::new();
    let switch = switch_progress(&archive, &mut switching)?;
    let Some(model) = archive.settings().embedding_model().map(str::to_owned) else {
        let mut text = String::from(
            "This archive has no embedding model yet, so it's searched by words only. Choose one \
             with `carrel index model NAME`.\n",
        );
        text.push_str(&switching);
        return Ok(Report::new(
            text,
            json!({ "model": null, "exists": false, "switch": switch }),
        ));
    };
    let index = Index::open(&archive, &model).map_err(|e| failure(e, &model))?;
    let plan = plan(&archive, index.as_ref()).map_err(|e| failure(e, &model))?;
    let behind = plan.pending.len();
    let worker = crate::worker::running(&archive).unwrap_or(false);
    let mut text = String::new();
    match &index {
        None => writeln!(
            text,
            "There's no meaning index for {model} yet, so search uses words only. Build it with \
             `carrel index rebuild`."
        )
        .unwrap(),
        Some(index) => {
            writeln!(
                text,
                "The meaning index uses {model}: {} of {} embedded, as {}; {behind} behind.",
                plan.current(),
                plural(plan.items, "item"),
                plural(index.vectors(), "vector"),
            )
            .unwrap();
            if index.rebuilding() {
                writeln!(
                    text,
                    "A rebuild stopped part way; `carrel index rebuild` carries on from there."
                )
                .unwrap();
            } else if behind > 0 && worker {
                writeln!(text, "The worker is catching up.").unwrap();
            } else if behind > 0 {
                writeln!(
                    text,
                    "The worker embeds them after the next change, or run `carrel index rebuild`."
                )
                .unwrap();
            }
        }
    }
    text.push_str(&switching);
    Ok(Report::new(
        text,
        json!({
            "model": model,
            "switch": switch,
            "exists": index.is_some(),
            "items": plan.items,
            "embedded": plan.current(),
            "behind": behind,
            "gone": plan.gone.len(),
            "vectors": index.as_ref().map_or(0, Index::vectors),
            "dimensions": index.as_ref().map_or(0, Index::dimensions),
            "rebuilding": index.as_ref().is_some_and(Index::rebuilding),
            "worker": worker,
        }),
    ))
}

/// Brings `archive`'s index for `model` up to date: reads the text of items not in the text
/// cache yet, then embeds what's behind, calling `progress` for each item. Returns how many
/// items were embedded.
///
/// # Errors
///
/// [`Failure`] if the archive can't be read, the model fails, or the index can't be written.
pub fn catch_up(
    archive: &Archive,
    registry: &Registry,
    index: &mut Index,
    progress: &mut dyn FnMut(usize, usize, Option<&str>),
) -> Result<usize, Failure> {
    let model = index.model().to_owned();
    crate::search::read_texts(archive, registry, false)?;
    let plan = plan(archive, Some(index)).map_err(|e| failure(e, &model))?;
    let items = plan.pending.len();
    index
        .embed(archive, registry, plan, progress)
        .map_err(|e| failure(e, &model))?;
    Ok(items)
}

/// `carrel index rebuild`: embeds every item again with the archive's embedding model, asked
/// for if there's none yet, showing progress. Interrupted, it carries on where it stopped
/// when run again.
///
/// # Errors
///
/// [`Failure`] if there's no archive or model, no provider here runs the model, or the index
/// can't be written.
pub fn rebuild(archive: Option<&Path>, json: bool) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let model = crate::models::archive_model(&archive)?;
    let (registry, _) = crate::providers::registry();
    let mut display = crate::progress::Display::new(format!("Embedding with {model}"), json);
    let report = rebuild_with(&archive, &registry, &model, &mut |done, total, item| {
        display.count("items", done, total, item);
    });
    display.finish();
    report
}

/// Embeds every item of `archive` again with `model`, the archive's embedding model, through
/// `registry`, calling `progress` for each item; what `carrel index rebuild` and the
/// interactive mode's Settings do.
///
/// # Errors
///
/// [`Failure`] if no provider in `registry` runs the model, or the index can't be written.
pub fn rebuild_with(
    archive: &Archive,
    registry: &Registry,
    model: &str,
    progress: &mut dyn FnMut(usize, usize, Option<&str>),
) -> Result<Report, Failure> {
    let mut index = Index::open_or_new(archive, model).map_err(|e| failure(e, model))?;
    let resumed = index.rebuilding();
    if !resumed {
        // Make sure the model answers before the old index is let go.
        embed_query(registry, model, "a test").map_err(|e| failure(e, model))?;
        index.start_rebuild();
        index.save().map_err(|e| failure(e, model))?;
    }
    let items = catch_up(archive, registry, &mut index, progress)?;
    let text = format!(
        "{} {} with {model}, as {}.\n",
        if resumed {
            "Carried on: embedded"
        } else {
            "Embedded"
        },
        plural(items, "item"),
        plural(index.vectors(), "vector"),
    );
    Ok(Report::new(
        text,
        json!({
            "model": model,
            "embedded": items,
            "items": index.len(),
            "vectors": index.vectors(),
            "dimensions": index.dimensions(),
            "resumed": resumed,
        }),
    ))
}

/// Writes how far the switch `archive` is part way through has got to `text`, and returns it
/// for `--json`: `null` when there's no switch.
fn switch_progress(archive: &Archive, text: &mut String) -> Result<Value, Failure> {
    let Some(Switch { from, to }) = switch::pending(archive) else {
        return Ok(Value::Null);
    };
    let index = Index::open(archive, &to).map_err(|e| failure(e, &to))?;
    let plan = plan(archive, index.as_ref()).map_err(|e| failure(e, &to))?;
    let serving = match &from {
        Some(from) => format!("search uses {from} until it's done"),
        None => "search uses words only until it's done".to_owned(),
    };
    writeln!(
        text,
        "Switching to {to}: {} of {} embedded; {serving}. `carrel index model {to}` carries on, \
         on any device that has {to}.",
        plan.current(),
        plural(plan.items, "item"),
    )
    .unwrap();
    Ok(json!({
        "from": from,
        "to": to,
        "items": plan.items,
        "embedded": plan.current(),
        "behind": plan.pending.len(),
    }))
}

/// Takes the archive's writer lease, runs `change` holding it, and lets it go.
fn holding<T>(
    archive: &Archive,
    holder: Holder,
    purpose: &str,
    change: impl FnOnce(&carrel_core::coordinator::WriterLease) -> Result<T, switch::SwitchError>,
) -> Result<T, Failure> {
    let lease = DiskLock::for_archive(archive).acquire(holder, purpose, DEFAULT_LEASE)?;
    let result = change(&lease);
    lease.release()?;
    Ok(result?)
}

/// `carrel index model`: the archive's embedding model, a switch in progress, and the models
/// this device can embed with, and those it could get.
///
/// # Errors
///
/// [`Failure`] if there's no archive or it can't be read.
pub fn models(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let current = archive.settings().embedding_model().map(str::to_owned);
    let mut text = match &current {
        Some(model) => format!("This archive is searched with {model}.\n"),
        None => "This archive has no embedding model yet.\n".to_owned(),
    };
    let switch = switch_progress(&archive, &mut text)?;
    let (registry, _) = crate::providers::registry();
    let here = offered_models(&registry);
    if here.is_empty() {
        text.push_str("\nNo provider on this device embeds.\n");
    } else {
        text.push_str("\nThis device can embed with:\n");
        for (id, provider, dimensions) in &here {
            let numbers = dimensions.map_or(String::new(), |d| format!(", {d} numbers"));
            let using = if current.as_deref() == Some(id) {
                " (in use)"
            } else {
                ""
            };
            writeln!(text, "  {id}  through {provider}{numbers}{using}").unwrap();
        }
    }
    let elsewhere: Vec<(&str, String)> = embedding_models()
        .filter(|m| !here.iter().any(|(id, _, _)| id == m.name))
        .filter_map(|m| Some((m.name, how_to_get(m.name)?)))
        .collect();
    if !elsewhere.is_empty() {
        text.push_str("\nNot on this device:\n");
        for (name, how) in &elsewhere {
            writeln!(text, "  {name}  ({how})").unwrap();
        }
    }
    text.push_str("\nSwitch with `carrel index model NAME`.\n");
    Ok(Report::new(
        text,
        json!({
            "model": current,
            "switch": switch,
            "available": here.iter().map(|(id, provider, dimensions)| json!({
                "model": id, "provider": provider, "dimensions": dimensions,
            })).collect::<Vec<_>>(),
            "elsewhere": elsewhere.iter().map(|(name, how)| json!({ "model": name, "how": how })).collect::<Vec<_>>(),
        }),
    ))
}

/// Why `model` can't be switched to on this device: no provider here offers it. Says which
/// models can be, and how to get it.
fn not_offered(registry: &Registry, model: &str) -> Failure {
    let here: Vec<String> = offered_models(registry)
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    let can = if here.is_empty() {
        "no provider here embeds".to_owned()
    } else {
        format!("this device can embed with {}", here.join(", "))
    };
    let how = how_to_get(model).map_or(String::new(), |how| format!("; {how}"));
    Failure::new(format!(
        "no provider on this device offers {model}, so it can't build its index; {can}{how}"
    ))
}

/// `carrel index model NAME` when NAME is already the archive's model: stops a switch to
/// another model, if one is under way.
fn keep(archive: &Archive, holder: Holder, model: &str) -> Result<Report, Failure> {
    if switch::pending(archive).is_none() {
        return Ok(Report::new(
            format!("This archive is already searched with {model}.\n"),
            json!({ "model": model, "switched": false, "stopped": null }),
        ));
    }
    let stopped = holding(archive, holder, "index model", |lease| {
        switch::cancel(archive, lease)
    })?;
    let to = stopped.as_deref().unwrap_or_default();
    Ok(Report::new(
        format!("Stopped switching to {to} and removed its index; the archive keeps {model}.\n"),
        json!({ "model": model, "switched": false, "stopped": stopped }),
    ))
}

/// `carrel index model NAME`: switches the archive to the embedding model NAME, which a
/// provider here must offer. Builds NAME's index beside the current one, showing progress,
/// while search keeps using the current one, then makes NAME the archive's model, for every
/// device, and removes the old index. Interrupted, it carries on when run again.
///
/// # Errors
///
/// [`Failure`] if there's no archive, no provider here offers NAME, it fails, or the index or
/// settings can't be written.
pub fn switch_to(archive: Option<&Path>, model: &str, json: bool) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let holder = holder(&Identity::load_or_create()?);
    let (registry, _) = crate::providers::registry();
    let model = model.trim();
    let mut display = crate::progress::Display::new(format!("Embedding with {model}"), json);
    let report = switch_with(
        &archive,
        &registry,
        holder,
        model,
        &mut |done, total, item| {
            display.count("items", done, total, item);
        },
    );
    display.finish();
    let report = report?;
    if let Some(replaced) = report.json["replaced"].as_str() {
        eprintln!("Gave up switching to {replaced}, and removed its index.");
    }
    Ok(report)
}

/// Switches `archive` to the embedding model `model` through `registry`, holding the writer
/// lease as `holder` while the switch is recorded and finished, and calling `progress` for
/// each item embedded; what `carrel index model NAME` and the interactive mode's Settings do.
/// When `model` is already the archive's, stops a switch to another model, if one is under
/// way.
///
/// # Errors
///
/// [`Failure`] if no provider in `registry` offers `model`, it fails, or the index or settings
/// can't be written.
pub fn switch_with(
    archive: &Archive,
    registry: &Registry,
    holder: Holder,
    model: &str,
    progress: &mut dyn FnMut(usize, usize, Option<&str>),
) -> Result<Report, Failure> {
    if archive.settings().embedding_model() == Some(model) {
        return keep(archive, holder, model);
    }
    if !offered(registry, model) {
        return Err(not_offered(registry, model));
    }
    // Make sure the model answers before anything is recorded.
    embed_query(registry, model, "a test").map_err(|e| failure(e, model))?;
    let (mut index, begun) = holding(archive, holder, "index model", |lease| {
        switch::begin(archive, model, lease)
    })?;
    let items = catch_up(archive, registry, &mut index, progress)?;
    let finished = holding(archive, holder, "index model", |lease| {
        switch::finish(archive, model, lease)
    })?;
    let mut text = format!(
        "{} {} with {model}, as {}. This archive is now searched with {model}, on every device \
         that has it.\n",
        if begun.resumed {
            "Carried on: embedded"
        } else {
            "Embedded"
        },
        plural(items, "item"),
        plural(index.vectors(), "vector"),
    );
    if let Some(from) = &finished.from {
        writeln!(text, "Removed {from}'s index.").unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "model": model,
            "from": finished.from,
            "switched": true,
            "embedded": items,
            "items": index.len(),
            "vectors": index.vectors(),
            "dimensions": index.dimensions(),
            "resumed": begun.resumed,
            "replaced": begun.replaced,
        }),
    ))
}
