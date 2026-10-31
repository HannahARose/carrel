//! `carrel expand ITEM`: expanding a compressed file that's already filed, with the same
//! checks as filing, as one undoable change (`docs/design/compressed.md`).

use std::path::Path;

use carrel_core::compressed::item::expand_item;
use carrel_core::coordinator::{Coordinator as _, DEFAULT_LEASE, DiskLock};
use serde_json::json;

use crate::commands::{find_item, holder, locate};
use crate::config::Identity;
use crate::{Failure, Report};

/// `carrel expand ITEM`.
///
/// # Errors
///
/// [`Failure`] if the item isn't found, isn't a compressed file, is kept compressed for a
/// reason (`--anyway` overrides software and size), or can't be expanded.
pub fn expand(archive: Option<&Path>, query: &str, anyway: bool) -> Result<Report, Failure> {
    let archive = locate(archive)?;
    let items = archive.items()?;
    let item = find_item(&archive, &items, query)?.clone();
    let identity = Identity::load_or_create()?;
    let lease =
        DiskLock::for_archive(&archive).acquire(holder(&identity), "expand", DEFAULT_LEASE)?;
    let result = expand_item(&archive, &item, anyway, &lease);
    lease.release()?;
    let expanded = result.map_err(|e| Failure::new(e.to_string()))?;
    let mut text = format!(
        "Expanded {} into {} ({} file{}, {})\n",
        expanded.from,
        expanded.to,
        expanded.files,
        if expanded.files == 1 { "" } else { "s" },
        carrel_core::human_size(expanded.size)
    );
    match &expanded.trash_problem {
        None => text.push_str(
            "The compressed file is in the Trash; undo it from History or with `carrel undo`, then put it back from the Trash.\n",
        ),
        Some(problem) => text.push_str(&format!(
            "The compressed file couldn't go to the Trash, so it's still at {}: {problem}\n",
            expanded.from
        )),
    }
    Ok(Report::new(
        text,
        json!({
            "item": item.id,
            "from": expanded.from,
            "to": expanded.to,
            "card": expanded.card,
            "kind": expanded.kind,
            "files": expanded.files,
            "size": expanded.size,
            "trash_problem": expanded.trash_problem,
            "commit": expanded.commit,
        }),
    ))
}
