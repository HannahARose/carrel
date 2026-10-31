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
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use carrel_core::index::manage::{self, Built, Stage, SwitchStatus, Switched};
use carrel_core::index::{Index, IndexError};
use carrel_core::models::embedding_models;
use carrel_core::providers::Registry;
use serde_json::{Value, json};

use crate::commands::{holder, locate, plural};
use crate::config::Identity;
use crate::progress::Display;
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
    manage::offered_models(registry)
        .into_iter()
        .map(|m| (m.model, m.provider, m.dimensions))
        .collect()
}

/// `carrel index status`: the archive's embedding model, how many items are embedded, and how
/// many are behind.
///
/// # Errors
///
/// [`Failure`] if there's no archive or it can't be read.
pub fn status(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let status = manage::status(&archive)?;
    let mut switching = String::new();
    let switch = switch_progress(status.switch.as_ref(), &mut switching);
    let Some(model) = &status.model else {
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
    let behind = status.behind;
    let worker = crate::worker::running(&archive).unwrap_or(false);
    let mut text = String::new();
    if status.exists {
        writeln!(
            text,
            "The meaning index uses {model}: {} of {} embedded, as {}; {behind} behind.",
            status.embedded,
            plural(status.items, "item"),
            plural(status.vectors, "vector"),
        )
        .unwrap();
        if status.rebuilding {
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
    } else {
        writeln!(
            text,
            "There's no meaning index for {model} yet, so search uses words only. Build it with \
             `carrel index rebuild`."
        )
        .unwrap();
    }
    text.push_str(&switching);
    Ok(Report::new(
        text,
        json!({
            "model": model,
            "switch": switch,
            "exists": status.exists,
            "items": status.items,
            "embedded": status.embedded,
            "behind": behind,
            "gone": status.gone,
            "vectors": status.vectors,
            "dimensions": status.dimensions,
            "rebuilding": status.rebuilding,
            "worker": worker,
        }),
    ))
}

/// Shows catching up with `model` on standard error: reading text, then embedding.
fn shown(model: &str, json: bool) -> impl FnMut(Stage, usize, usize, Option<&str>) {
    let mut reading: Option<Display> = None;
    let mut embedding = Display::new(format!("Embedding with {model}"), json);
    move |stage, done, total, item| match stage {
        Stage::Reading => reading
            .get_or_insert_with(|| Display::new("Reading text", json))
            .count("items", done, total, item),
        Stage::Embedding => {
            if let Some(mut reading) = reading.take() {
                reading.finish();
            }
            embedding.count("items", done, total, item);
        }
    }
}

/// Brings `archive`'s index for `model` up to date: reads the text of items not in the text
/// cache yet, then embeds what's behind, showing progress. Returns how many items were
/// embedded.
///
/// # Errors
///
/// [`Failure`] if the archive can't be read, the model fails, or the index can't be written.
pub fn catch_up(
    archive: &Archive,
    registry: &Registry,
    index: &mut Index,
    json: bool,
) -> Result<usize, Failure> {
    let model = index.model().to_owned();
    Ok(manage::catch_up(
        archive,
        registry,
        index,
        &mut shown(&model, json),
    )?)
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
    let built = manage::rebuild(&archive, &registry, &model, &mut shown(&model, json))?;
    let text = format!(
        "{} {} with {model}, as {}.\n",
        if built.resumed {
            "Carried on: embedded"
        } else {
            "Embedded"
        },
        plural(built.embedded, "item"),
        plural(built.vectors, "vector"),
    );
    Ok(Report::new(
        text,
        json!({
            "model": model,
            "embedded": built.embedded,
            "items": built.items,
            "vectors": built.vectors,
            "dimensions": built.dimensions,
            "resumed": built.resumed,
        }),
    ))
}

/// Writes how far `switch` has got to `text`, and returns it for `--json`: `null` when
/// there's no switch.
fn switch_progress(switch: Option<&SwitchStatus>, text: &mut String) -> Value {
    let Some(switch) = switch else {
        return Value::Null;
    };
    let to = &switch.to;
    let serving = match &switch.from {
        Some(from) => format!("search uses {from} until it's done"),
        None => "search uses words only until it's done".to_owned(),
    };
    writeln!(
        text,
        "Switching to {to}: {} of {} embedded; {serving}. `carrel index model {to}` carries on, \
         on any device that has {to}.",
        switch.embedded,
        plural(switch.items, "item"),
    )
    .unwrap();
    json!({
        "from": switch.from,
        "to": to,
        "items": switch.items,
        "embedded": switch.embedded,
        "behind": switch.behind,
    })
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
    let switch = switch_progress(manage::switch_status(&archive)?.as_ref(), &mut text);
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

/// `carrel index model NAME`: switches the archive to the embedding model NAME, which a
/// provider here must offer. Builds NAME's index beside the current one, showing progress,
/// while search keeps using the current one, then makes NAME the archive's model, for every
/// device, and removes the old index. Interrupted, it carries on when run again. NAME already
/// the archive's model, it stops a switch to another model, if one is under way.
///
/// # Errors
///
/// [`Failure`] if there's no archive, no provider here offers NAME, it fails, or the index or
/// settings can't be written.
pub fn switch_to(archive: Option<&Path>, model: &str, json: bool) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let model = model.trim();
    let (registry, _) = crate::providers::registry();
    let identity = Identity::load_or_create()?;
    let switched = manage::switch_to(
        &archive,
        &registry,
        model,
        &mut || {
            DiskLock::for_archive(&archive).acquire(holder(&identity), "index model", DEFAULT_LEASE)
        },
        &mut shown(model, json),
    )?;
    let (from, replaced, built) = match switched {
        Switched::Already => {
            return Ok(Report::new(
                format!("This archive is already searched with {model}.\n"),
                json!({ "model": model, "switched": false, "stopped": null }),
            ));
        }
        Switched::Stopped(to) => {
            return Ok(Report::new(
                format!(
                    "Stopped switching to {to} and removed its index; the archive keeps {model}.\n"
                ),
                json!({ "model": model, "switched": false, "stopped": to }),
            ));
        }
        Switched::Done {
            from,
            replaced,
            built,
        } => (from, replaced, built),
    };
    if let Some(replaced) = &replaced {
        eprintln!("Gave up switching to {replaced}, and removed its index.");
    }
    let Built {
        embedded,
        items,
        vectors,
        dimensions,
        resumed,
    } = built;
    let mut text = format!(
        "{} {} with {model}, as {}. This archive is now searched with {model}, on every device \
         that has it.\n",
        if resumed {
            "Carried on: embedded"
        } else {
            "Embedded"
        },
        plural(embedded, "item"),
        plural(vectors, "vector"),
    );
    if let Some(from) = &from {
        writeln!(text, "Removed {from}'s index.").unwrap();
    }
    Ok(Report::new(
        text,
        json!({
            "model": model,
            "from": from,
            "switched": true,
            "embedded": embedded,
            "items": items,
            "vectors": vectors,
            "dimensions": dimensions,
            "resumed": resumed,
            "replaced": replaced,
        }),
    ))
}
