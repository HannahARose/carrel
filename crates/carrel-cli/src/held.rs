//! Restoring from the holding folder (`docs/design/sync-and-mobile.md`, "Devices without
//! Kopia"): on a device without backups, the content an operation replaced is kept there until
//! a snapshot has it, and `carrel backup restore ITEM --held` brings it back beside the
//! archive.

use std::path::Path;

use carrel_core::archive::{Archive, ItemRef};
use carrel_core::holding::{self, Held};
use serde_json::json;

use crate::{Failure, Report};

/// The line saying the version an operation replaced is in the holding folder, and how to
/// bring it back.
pub fn kept_line(held: &Held) -> String {
    format!(
        "The version it replaced is kept in the holding folder until a snapshot has it; restore it with `carrel backup restore {} --held {}`.\n",
        held.item
            .map_or_else(|| held.title.clone(), |id| id.to_string()),
        held.id
    )
}

/// `carrel backup restore ITEM --held [ID]`, or without backups: the kept version `id` of
/// `item`, or its newest, copied to `to` or beside the archive. Nothing in the archive
/// changes.
///
/// # Errors
///
/// [`Failure`] if nothing of the item is held, `id` isn't a version of it, or it can't be
/// copied.
pub fn restore(
    archive: &Archive,
    item: &ItemRef,
    id: Option<&str>,
    to: Option<&Path>,
) -> Result<Report, Failure> {
    let held = match id {
        Some(id) => {
            let held = holding::find(archive, id).map_err(Failure::new)?;
            if held.item != Some(item.id) {
                return Err(Failure::new(format!(
                    "{id} in the holding folder isn't a version of {}",
                    item.card
                )));
            }
            held
        }
        None => holding::list(archive)
            .into_iter()
            .rfind(|h| h.item == Some(item.id))
            .ok_or_else(|| {
                Failure::new(format!(
                    "nothing of {} is in the holding folder{}",
                    item.card,
                    if archive.settings().backups.is_none() {
                        ", and no backups are set up (see `carrel backup setup`)"
                    } else {
                        "; leave out --held to restore from a backup"
                    }
                ))
            })?,
    };
    let target = to.map_or_else(|| holding::restore_beside(archive, &held), Path::to_owned);
    holding::restore(archive, &held, &target).map_err(|e| Failure::io(&target, &e))?;
    Ok(Report::new(
        format!(
            "Restored {}, kept when it was {} ({}), from the holding folder to {}\nNothing in the archive changed; compare, then move it in yourself if you want it.\n",
            held.name,
            held.why.describe(),
            held.at.get(..10).unwrap_or_default(),
            target.display()
        ),
        json!({"item": item.id, "held": held.id, "to": target}),
    ))
}
