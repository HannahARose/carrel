//! `sync.keep_downloaded`: a collection's setting, in its design document as
//! `sync: {keep_downloaded: true}`, that keeps its items downloaded on every device, for
//! offline use or a big screen. Off by default. A subcollection follows the nearest collection
//! above it that says, unless it says otherwise itself.

use super::{SyncError, io};
use crate::archive::{Archive, Collection};
use crate::coordinator::WriterLease;

/// The section and key in a design document.
const SECTION: &str = "sync";
const KEY: &str = "keep_downloaded";

/// The collection's own `sync.keep_downloaded`, if its design document sets it.
#[must_use]
pub fn own_setting(collection: &Collection) -> Option<bool> {
    collection.doc.extra.get(SECTION)?.get(KEY)?.as_bool()
}

/// Whether the collection at `path` keeps its items downloaded: its own setting, else the
/// nearest collection's above it, else off.
#[must_use]
pub fn keeps_downloaded(collections: &[Collection], path: &str) -> bool {
    let mut at = path.trim_matches('/');
    loop {
        if let Some(on) = collections
            .iter()
            .find(|c| c.path == at)
            .and_then(own_setting)
        {
            return on;
        }
        match at.rsplit_once('/') {
            Some((parent, _)) => at = parent,
            None => return false,
        }
    }
}

/// Sets the collection at `path`'s `sync.keep_downloaded`, or with `None` clears it so the
/// collection above decides, and records it as one change holding `lease`. Returns the
/// history commit, or `None` when it was already so.
///
/// # Errors
///
/// [`SyncError::Invalid`] if there's no such collection or its design document can't be read,
/// or an error writing it or the history.
pub fn set_keep_downloaded(
    archive: &Archive,
    path: &str,
    on: Option<bool>,
    lease: &WriterLease,
) -> Result<Option<String>, SyncError> {
    let path = path.trim().trim_matches('/');
    let collections = archive.collections()?;
    let collection = collections
        .iter()
        .find(|c| c.path.eq_ignore_ascii_case(path))
        .ok_or_else(|| SyncError::Invalid(format!("there's no collection {path}")))?;
    if own_setting(collection) == on {
        return Ok(None);
    }
    let document = archive.root().join(&collection.path).join("_collection.md");
    let text = std::fs::read_to_string(&document).map_err(io(&document))?;
    let invalid = |e: String| SyncError::Invalid(format!("{}: {e}", collection.path));
    let split = crate::frontmatter::split(&text).map_err(|e| invalid(e.to_string()))?;
    let mut doc: crate::documents::CollectionDoc =
        serde_saphyr::from_str(split.yaml).map_err(|e| invalid(e.to_string()))?;
    let mut section = doc
        .extra
        .shift_remove(SECTION)
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    match on {
        Some(on) => {
            section.insert(KEY.into(), on.into());
        }
        None => {
            section.remove(KEY);
        }
    }
    if !section.is_empty() {
        doc.extra
            .insert(SECTION.into(), serde_json::Value::Object(section));
    }
    let yaml = serde_saphyr::to_string(&doc).map_err(|e| invalid(e.to_string()))?;
    std::fs::write(&document, crate::frontmatter::join(&yaml, split.body))
        .map_err(io(&document))?;
    let what = match on {
        Some(true) => "on",
        Some(false) => "off",
        None => "the collection above's",
    };
    let change = crate::history::Change::new(
        crate::history::Operation::Edit,
        format!("Set sync.keep_downloaded for {} to {what}", collection.path),
        lease.info().person,
        lease.info().device,
    );
    let archive = Archive::open(archive.root())?;
    Ok(crate::history::History::open(&archive)?.commit(&archive, &change, lease)?)
}

/// What a pass of [`keep_downloaded_pass`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeptPass {
    /// The collections kept downloaded.
    pub collections: Vec<String>,
    /// The files it downloaded, relative to the archive root.
    pub downloaded: Vec<String>,
    /// The files it couldn't download, and why.
    pub failed: Vec<(String, String)>,
}

/// Downloads every evicted file in the items of collections that keep theirs downloaded, as
/// the agent does now and then.
///
/// # Errors
///
/// [`SyncError::Archive`] if the archive's collections or cards can't be read. Files that
/// can't be downloaded are reported in [`KeptPass::failed`].
pub fn keep_downloaded_pass(archive: &Archive) -> Result<KeptPass, SyncError> {
    let collections = archive.collections()?;
    let mut pass = KeptPass {
        collections: collections
            .iter()
            .filter(|c| keeps_downloaded(&collections, &c.path))
            .map(|c| c.path.clone())
            .collect(),
        ..KeptPass::default()
    };
    if pass.collections.is_empty() {
        return Ok(pass);
    }
    let provider = super::provider();
    let root = archive.root();
    for item in archive.items()? {
        let collection = item.card.rsplit_once('/').map_or("", |(c, _)| c);
        let Some(contents) = item.item.as_ref() else {
            continue;
        };
        if !pass.collections.iter().any(|c| c == collection) {
            continue;
        }
        for file in super::not_downloaded_in(&root.join(contents), provider.as_ref()) {
            let rel = super::relative(root, &file);
            match provider.download(&file) {
                Ok(()) => pass.downloaded.push(rel),
                Err(e) => pass.failed.push((rel, e.to_string())),
            }
        }
    }
    Ok(pass)
}
