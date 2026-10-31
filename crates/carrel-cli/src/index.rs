//! `carrel index`: the embedding index search finds items by meaning with
//! (`docs/design/search.md`): how far behind it is, and rebuilding it.
//!
//! The worker keeps it current in the background after every change; these commands report on
//! it and rebuild it by hand. Only the archive's embedding model is used, never another.

use std::fmt::Write as _;
use std::path::Path;

use carrel_core::archive::Archive;
use carrel_core::index::{Index, IndexError, embed_query, plan};
use carrel_core::models::find;
use carrel_core::providers::{Capability, ProviderError, Registry};
use serde_json::json;

use crate::commands::{locate, plural};
use crate::{Failure, Report};

/// `error`, in words, with how to get `model` when no provider here has it.
pub fn failure(error: IndexError, model: &str) -> Failure {
    match error {
        IndexError::Provider(e @ ProviderError::Unavailable(_)) if find(model).is_some() => {
            Failure::new(format!(
                "{e}; download it with `carrel models download {model}`"
            ))
        }
        e => Failure::new(e.to_string()),
    }
}

/// Whether a provider on this device offers the embedding model `model`.
pub fn offered(registry: &Registry, model: &str) -> bool {
    registry
        .ranked(Capability::Embed)
        .iter()
        .any(|(_, offer)| offer.id == model)
}

/// `carrel index status`: the archive's embedding model, how many items are embedded, and how
/// many are behind.
///
/// # Errors
///
/// [`Failure`] if there's no archive or it can't be read.
pub fn status(archive: Option<&Path>) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let Some(model) = archive.settings().embedding_model().map(str::to_owned) else {
        return Ok(Report::new(
            "This archive has no embedding model yet, so it's searched by words only. Choose one \
             with `carrel settings set embedding.model NAME`, or run `carrel index rebuild`.\n",
            json!({ "model": null, "exists": false }),
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
    Ok(Report::new(
        text,
        json!({
            "model": model,
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
    let mut index = Index::open_or_new(&archive, &model).map_err(|e| failure(e, &model))?;
    let resumed = index.rebuilding();
    if !resumed {
        // Make sure the model answers before the old index is let go.
        embed_query(&registry, &model, "a test").map_err(|e| failure(e, &model))?;
        index.start_rebuild();
        index.save().map_err(|e| failure(e, &model))?;
    }
    let mut display = crate::progress::Display::new(format!("Embedding with {model}"), json);
    let items = catch_up(&archive, &registry, &mut index, &mut |done, total, item| {
        display.count("items", done, total, item);
    })?;
    display.finish();
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
