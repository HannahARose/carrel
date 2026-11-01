//! What the crawler concluded about each item, kept per device in
//! `_System/devices/<device-id>/crawler.json` (FORMAT.md §7.10), so two devices crawling at once
//! never write the same file. Reading them merges every device's, the newest verdict for each
//! item winning.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::findings::Finding;
use crate::archive::Archive;
use crate::card::{Extra, Id};

/// The file, in a device's folder.
const FILE: &str = "crawler.json";

/// How many items of a collection must fail before it's a signal.
pub const MISFITS_LEAST: usize = 3;

/// What the crawler concluded about one item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    /// The collection it was in, by path.
    pub collection: String,
    /// Whether it fits there.
    pub fits: bool,
    /// Where it would go instead, by path, when it doesn't fit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub better: Option<String>,
    /// How sure the model was, from 0 to 1.
    pub confidence: f64,
    /// Why, in the model's words.
    #[serde(default)]
    pub reason: String,
    /// The model, as `provider/offer`.
    pub model: String,
    /// When, as RFC 3339.
    pub judged: String,
    /// Findings the owner rejected, so they aren't proposed again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub declined: Vec<Finding>,
    /// The drafting version current when it was given ([`crate::ingest::card::DRAFTING`]);
    /// unset before drafting versions were kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drafting: Option<u32>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Verdict {
    /// When it was given.
    #[must_use]
    pub fn at(&self) -> Option<jiff::Timestamp> {
        self.judged.parse().ok()
    }
}

/// One device's verdicts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Verdicts {
    /// When a pass last ran on the device, as RFC 3339.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_pass: Option<String>,
    /// Each item's verdict, by identifier.
    #[serde(default)]
    pub items: BTreeMap<Id, Verdict>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Verdicts {
    /// Where `device`'s verdicts for `archive` are kept.
    #[must_use]
    pub fn path(archive: &Archive, device: Id) -> PathBuf {
        archive
            .root()
            .join("_System/devices")
            .join(device.to_string())
            .join(FILE)
    }

    /// `device`'s verdicts, or none yet. A file that can't be read counts as none: verdicts
    /// are only ever a guide, and the crawler gives them again.
    #[must_use]
    pub fn load(archive: &Archive, device: Id) -> Self {
        std::fs::read_to_string(Self::path(archive, device))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Saves `device`'s verdicts, replacing the file whole.
    ///
    /// # Errors
    ///
    /// Any error writing it.
    pub fn save(&self, archive: &Archive, device: Id) -> std::io::Result<()> {
        let path = Self::path(archive, device);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)? + "\n";
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, text)?;
        std::fs::rename(&temp, &path)
    }
}

/// Every device's verdicts for `archive`, merged: the newest for each item.
#[must_use]
pub fn all(archive: &Archive) -> BTreeMap<Id, Verdict> {
    let mut merged: BTreeMap<Id, Verdict> = BTreeMap::new();
    let Ok(devices) = std::fs::read_dir(archive.root().join("_System/devices")) else {
        return merged;
    };
    for device in devices.filter_map(Result::ok) {
        let Ok(text) = std::fs::read_to_string(device.path().join(FILE)) else {
            continue;
        };
        let Ok(verdicts) = serde_json::from_str::<Verdicts>(&text) else {
            continue;
        };
        for (id, mut verdict) in verdicts.items {
            match merged.get_mut(&id) {
                None => {
                    merged.insert(id, verdict);
                }
                Some(have) => {
                    // What the owner declined on any device stays declined.
                    let mut declined = std::mem::take(&mut have.declined);
                    for finding in verdict.declined.drain(..) {
                        if !declined.contains(&finding) {
                            declined.push(finding);
                        }
                    }
                    if verdict.at() > have.at() {
                        *have = verdict;
                    }
                    have.declined = declined;
                }
            }
        }
    }
    merged
}

/// The collections whose items keep failing, most failures first: each with how many failed
/// and how many were judged. A collection counts once at least [`MISFITS_LEAST`] of its items
/// failed and they're at least a third of those judged. Holding doesn't count, since nothing
/// is meant to stay there.
#[must_use]
pub fn misfits(verdicts: &BTreeMap<Id, Verdict>) -> Vec<(String, usize, usize)> {
    let mut by_collection: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for verdict in verdicts.values() {
        let counts = by_collection.entry(&verdict.collection).or_default();
        counts.1 += 1;
        if !verdict.fits {
            counts.0 += 1;
        }
    }
    let mut found: Vec<(String, usize, usize)> = by_collection
        .into_iter()
        .filter(|(path, (failed, judged))| {
            *path != "Holding" && *failed >= MISFITS_LEAST && failed * 3 >= *judged
        })
        .map(|(path, (failed, judged))| (path.to_owned(), failed, judged))
        .collect();
    found.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(collection: &str, fits: bool) -> Verdict {
        Verdict {
            collection: collection.into(),
            fits,
            better: None,
            confidence: 0.8,
            reason: String::new(),
            model: "m/m".into(),
            judged: "2026-11-01T10:00:00Z".into(),
            declined: Vec::new(),
            drafting: None,
            extra: Extra::new(),
        }
    }

    #[test]
    fn a_collection_whose_items_keep_failing_is_a_misfit() {
        let mut verdicts = BTreeMap::new();
        for (n, (collection, fits)) in [
            ("Receipts", false),
            ("Receipts", false),
            ("Receipts", false),
            ("Receipts", true),
            ("Receipts", true),
            ("Home", false),
            ("Home", false),
            ("Holding", false),
            ("Holding", false),
            ("Holding", false),
            ("Work", false),
            ("Work", false),
            ("Work", false),
        ]
        .into_iter()
        .chain(std::iter::repeat_n(("Work", true), 7))
        .enumerate()
        {
            verdicts.insert(
                Id(uuid::Uuid::from_u128(n as u128 + 1)),
                verdict(collection, fits),
            );
        }
        // Work: 3 of 10 failed, under a third; Home: only 2; Holding never counts.
        assert_eq!(misfits(&verdicts), [("Receipts".to_owned(), 3, 5)]);
    }
}
