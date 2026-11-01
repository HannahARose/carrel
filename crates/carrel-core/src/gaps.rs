//! What Carrel couldn't do on this device (#455): file types nothing read, extraction and
//! drafting that failed, the crawler's model failing, a provider missing for a capability, and
//! the missing features the owner reports by hand, so they can be reported to development.
//!
//! It's kept in Carrel's own folder on each device as `gaps.json`, next to the decision log
//! ([`crate::decisions`]) and `unreadable.json` ([`crate::formats`]), which feeds it. Like
//! them it records only kinds, media types, extensions, provider and stage names, Carrel's
//! version and counts, **never titles, text, paths or file names**. The one exception is the
//! words the owner writes themself when reporting a missing feature.
//!
//! One gap is one entry: sightings of the same kind, type and stage add to its count of items.
//! Nothing is sent anywhere. [`draft::IssueDraft`] builds a GitHub issue draft from a gap, as a
//! prefilled new-issue address the owner opens, reads, edits and submits themself; the gap
//! then keeps the issue's address the owner gives back ([`GapLog::mark_reported`]).
//!
//! Nothing is written until an interface calls [`start_in`] (through
//! [`crate::decisions::start`], as the command line, the interactive mode and the apps do as
//! they open), so library code and tests never write the owner's log.

pub mod draft;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::card::Extra;
use crate::formats::Unreadable;
use crate::ingest::text::Unread;

/// The file in the configuration folder that holds the log.
pub const FILE: &str = "gaps.json";

/// The most characters of the owner's own words a gap keeps.
pub const MAX_WORDS: usize = 2000;

/// The kind of thing Carrel couldn't do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GapKind {
    /// Nothing on this device reads the file type.
    UnreadFormat,
    /// Every reader that tried the file type failed.
    ExtractionFailed,
    /// Drafting an item's card failed for good.
    DraftingFailed,
    /// The crawler's model failed on an item, or its answer wasn't usable.
    CrawlerFailed,
    /// No provider on this device offers a capability that was needed.
    NoProvider,
    /// The librarian concluded that fixing a card needs something Carrel can't do (#454).
    LibrarianCantHelp,
    /// The owner reported a missing feature themself.
    OwnerReported,
}

impl GapKind {
    /// In words, such as `no reader for the file type`.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::UnreadFormat => "no reader for the file type",
            Self::ExtractionFailed => "reading the file's text failed",
            Self::DraftingFailed => "drafting the card failed",
            Self::CrawlerFailed => "the crawler couldn't judge the item",
            Self::NoProvider => "no provider for a capability",
            Self::LibrarianCantHelp => "the librarian couldn't fix the card",
            Self::OwnerReported => "a missing feature the owner reported",
        }
    }

    /// Its name, as `--json` and `gaps.json` give it, such as `unread-format`.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::UnreadFormat => "unread-format",
            Self::ExtractionFailed => "extraction-failed",
            Self::DraftingFailed => "drafting-failed",
            Self::CrawlerFailed => "crawler-failed",
            Self::NoProvider => "no-provider",
            Self::LibrarianCantHelp => "librarian-cant-help",
            Self::OwnerReported => "owner-reported",
        }
    }
}

/// One sighting of a gap: what couldn't be done, to what type of item, where. It holds no
/// names: `stage` is a provider, offer or stage name, never text from an item or a model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sighting {
    /// What couldn't be done.
    pub kind: GapKind,
    /// The item's media type, such as `model/3mf`; empty when there's no item.
    pub media_type: String,
    /// The item's extension, lowercased and without the dot; empty if it has none.
    pub extension: String,
    /// The provider or stage that failed, such as `text extraction` or `llama/qwen3-4b`.
    pub stage: String,
}

impl Sighting {
    /// A sighting of `kind` for a file of `media_type` and `extension`, at `stage`.
    #[must_use]
    pub fn new(kind: GapKind, media_type: &str, extension: &str, stage: &str) -> Self {
        Self {
            kind,
            media_type: media_type.to_owned(),
            extension: extension.to_lowercase(),
            stage: stage.to_owned(),
        }
    }
}

/// One gap: every sighting of the same kind, type and stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gap {
    /// Its number on this device, from 1, for `carrel report gaps --reported N`.
    pub number: u32,
    /// What couldn't be done.
    pub kind: GapKind,
    /// The items' media type; empty when there's no item.
    #[serde(default)]
    pub media_type: String,
    /// The items' extension, without the dot; empty if they have none.
    #[serde(default)]
    pub extension: String,
    /// The provider or stage that failed.
    #[serde(default)]
    pub stage: String,
    /// How many items it has affected.
    pub items: u64,
    /// The day it was first seen, `YYYY-MM-DD`.
    pub first_seen: String,
    /// The day it was last seen.
    pub last_seen: String,
    /// The version of Carrel that first saw it.
    pub first_version: String,
    /// The version of Carrel that last saw it.
    pub last_version: String,
    /// The owner's own words, for a missing feature they reported themself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub words: Option<String>,
    /// The address of the issue the owner reported it in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported: Option<String>,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl Gap {
    /// Whether it's the gap `sighting` belongs to, with the owner's `words`.
    fn matches(&self, sighting: &Sighting, words: Option<&str>) -> bool {
        self.kind == sighting.kind
            && self.media_type == sighting.media_type
            && self.extension == sighting.extension
            && self.stage == sighting.stage
            && self.words.as_deref() == words
    }

    /// The type of item it's about in words: `.3mf (model/3mf)`, or an empty string.
    #[must_use]
    pub fn file_type(&self) -> String {
        match (self.extension.is_empty(), self.media_type.is_empty()) {
            (true, true) => String::new(),
            (true, false) => self.media_type.clone(),
            (false, true) => format!(".{}", self.extension),
            (false, false) => format!(".{} ({})", self.extension, self.media_type),
        }
    }

    /// A one-line description, such as `no reader for the file type: .3mf (model/3mf), at
    /// text extraction`.
    #[must_use]
    pub fn summary(&self) -> String {
        let mut text = self.kind.describe().to_owned();
        let file_type = self.file_type();
        if !file_type.is_empty() {
            text.push_str(": ");
            text.push_str(&file_type);
        }
        if !self.stage.is_empty() {
            text.push_str(", at ");
            text.push_str(&self.stage);
        }
        text
    }

    /// Whether a Carrel newer than the one that last saw it is running, `current`, so the
    /// gap may have been addressed: it hasn't been seen since.
    #[must_use]
    pub fn not_seen_since_update(&self, current: &str) -> bool {
        newer(current, &self.last_version)
    }
}

/// Whether the version `a` is newer than `b`, comparing their numbers in order.
fn newer(a: &str, b: &str) -> bool {
    let numbers = |v: &str| -> Vec<u64> {
        v.split(['.', '-', '+'])
            .map_while(|p| p.parse().ok())
            .collect()
    };
    numbers(a) > numbers(b)
}

/// Why the log couldn't do what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GapError {
    /// There's no gap with that number.
    NoGap(u32),
    /// The issue's address isn't a web address.
    NotAnAddress(String),
    /// The owner's words were empty.
    NoWords,
    /// The log couldn't be read or written.
    Io(String),
}

impl std::fmt::Display for GapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoGap(n) => write!(f, "there's no gap {n}"),
            Self::NotAnAddress(a) => write!(
                f,
                "“{a}” isn't the issue's web address, such as https://github.com/Horizon-Labs/carrel/issues/123"
            ),
            Self::NoWords => f.write_str("say what Carrel should be able to do"),
            Self::Io(e) => write!(f, "the gap log couldn't be saved: {e}"),
        }
    }
}

impl std::error::Error for GapError {}

/// This device's log of gaps: `gaps.json` in Carrel's folder.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GapLog {
    /// The gaps, most items first.
    #[serde(default)]
    pub gaps: Vec<Gap>,
    /// The number the next gap gets.
    #[serde(default)]
    pub next: u32,
    /// Keys this version doesn't know.
    #[serde(flatten)]
    pub extra: Extra,
}

impl GapLog {
    /// The saved log at `path`, or an empty one.
    #[must_use]
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Saves it at `path`.
    ///
    /// # Errors
    ///
    /// An error writing the file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, text + "\n")
    }

    /// The gap numbered `number`.
    #[must_use]
    pub fn get(&self, number: u32) -> Option<&Gap> {
        self.gaps.iter().find(|g| g.number == number)
    }

    /// Counts `items` more items for `sighting`, on `today`, seen by Carrel `version`. The
    /// gap's number.
    pub fn note(&mut self, sighting: &Sighting, items: u64, today: &str, version: &str) -> u32 {
        self.note_with(sighting, None, items, today, version)
    }

    /// Records a missing feature the owner reports themself, in their own `words`, about an
    /// item `sighting` describes (or none). The same words about the same type add to the gap
    /// already there. The gap's number.
    ///
    /// # Errors
    ///
    /// [`GapError::NoWords`] if `words` are empty.
    pub fn report_by_hand(
        &mut self,
        sighting: &Sighting,
        words: &str,
        today: &str,
        version: &str,
    ) -> Result<u32, GapError> {
        let words: String = words.trim().chars().take(MAX_WORDS).collect();
        if words.is_empty() {
            return Err(GapError::NoWords);
        }
        let sighting = Sighting {
            kind: GapKind::OwnerReported,
            ..sighting.clone()
        };
        Ok(self.note_with(&sighting, Some(&words), 1, today, version))
    }

    fn note_with(
        &mut self,
        sighting: &Sighting,
        words: Option<&str>,
        items: u64,
        today: &str,
        version: &str,
    ) -> u32 {
        let number = if let Some(gap) = self.gaps.iter_mut().find(|g| g.matches(sighting, words)) {
            gap.items += items;
            today.clone_into(&mut gap.last_seen);
            version.clone_into(&mut gap.last_version);
            gap.number
        } else {
            let number = self.take_number();
            self.gaps.push(Gap {
                number,
                kind: sighting.kind,
                media_type: sighting.media_type.clone(),
                extension: sighting.extension.clone(),
                stage: sighting.stage.clone(),
                items,
                first_seen: today.to_owned(),
                last_seen: today.to_owned(),
                first_version: version.to_owned(),
                last_version: version.to_owned(),
                words: words.map(ToOwned::to_owned),
                reported: None,
                extra: Extra::new(),
            });
            number
        };
        self.sort();
        number
    }

    fn take_number(&mut self) -> u32 {
        let highest = self.gaps.iter().map(|g| g.number).max().unwrap_or(0);
        let number = self.next.max(highest + 1).max(1);
        self.next = number + 1;
        number
    }

    fn sort(&mut self) {
        self.gaps.sort_by(|a, b| {
            b.items
                .cmp(&a.items)
                .then(b.last_seen.cmp(&a.last_seen))
                .then(a.number.cmp(&b.number))
        });
    }

    /// Records that gap `number` was reported in the issue at `address`, which the owner
    /// pastes back after submitting the draft. An empty address clears it.
    ///
    /// # Errors
    ///
    /// [`GapError::NoGap`] or [`GapError::NotAnAddress`].
    pub fn mark_reported(&mut self, number: u32, address: &str) -> Result<(), GapError> {
        let address = address.trim();
        let valid = address.is_empty()
            || ((address.starts_with("https://") || address.starts_with("http://"))
                && !address.contains(char::is_whitespace));
        if !valid {
            return Err(GapError::NotAnAddress(address.to_owned()));
        }
        let gap = self
            .gaps
            .iter_mut()
            .find(|g| g.number == number)
            .ok_or(GapError::NoGap(number))?;
        gap.reported = (!address.is_empty()).then(|| address.to_owned());
        Ok(())
    }

    /// Brings in the types `unreadable` ([`crate::formats`]) couldn't read: each type nothing
    /// read, or whose readers failed, is a gap with at least as many items. Whether anything
    /// changed.
    pub fn bring_in_unreadable(&mut self, unreadable: &Unreadable, version: &str) -> bool {
        let mut changed = false;
        for entry in &unreadable.entries {
            let Some(sighting) = unread_sighting(&entry.media_type, &entry.extension, entry.why)
            else {
                continue;
            };
            if let Some(gap) = self.gaps.iter_mut().find(|g| g.matches(&sighting, None)) {
                if gap.items < entry.count {
                    gap.items = entry.count;
                    changed = true;
                }
                if gap.first_seen > entry.first_seen {
                    gap.first_seen.clone_from(&entry.first_seen);
                    changed = true;
                }
                if gap.last_seen < entry.last_seen {
                    gap.last_seen.clone_from(&entry.last_seen);
                    changed = true;
                }
            } else {
                let number = self.note(&sighting, entry.count, &entry.last_seen, version);
                if let Some(gap) = self.gaps.iter_mut().find(|g| g.number == number) {
                    gap.first_seen.clone_from(&entry.first_seen);
                }
                changed = true;
            }
        }
        if changed {
            self.sort();
        }
        changed
    }
}

/// The stage a file's text is read at.
pub const EXTRACTION: &str = "text extraction";

/// The sighting for a file of `media_type` and `extension` whose text wasn't read for `why`:
/// none when it was read but held no text, which isn't something Carrel lacks.
#[must_use]
pub fn unread_sighting(media_type: &str, extension: &str, why: Unread) -> Option<Sighting> {
    let kind = match why {
        Unread::NoReader => GapKind::UnreadFormat,
        Unread::Failed => GapKind::ExtractionFailed,
        Unread::Empty => return None,
    };
    Some(Sighting::new(kind, media_type, extension, EXTRACTION))
}

/// Where gaps are recorded, once [`start_in`] has been called.
static LOG: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The log's file in the configuration folder `config`.
#[must_use]
pub fn file_in(config: &Path) -> PathBuf {
    config.join(FILE)
}

/// Starts recording gaps in the configuration folder `config`. [`crate::decisions::start`]
/// calls it, so interfaces needn't.
pub fn start_in(config: &Path) {
    if let Ok(mut log) = LOG.lock() {
        *log = Some(file_in(config));
    }
}

/// Stops recording gaps until [`start_in`] is called again.
pub fn stop() {
    if let Ok(mut log) = LOG.lock() {
        *log = None;
    }
}

/// The file gaps are being recorded in, if recording has started.
#[must_use]
pub fn recording_to() -> Option<PathBuf> {
    LOG.lock().ok().and_then(|l| l.clone())
}

/// Today, as gaps record it.
fn today() -> String {
    jiff::Zoned::now().date().to_string()
}

/// Records `items` items for each of `sightings` in this device's log, if recording has
/// started. Never fails: a log that can't be written is skipped, as the owner's work matters
/// more than the report.
pub fn record(sightings: &[(Sighting, u64)]) {
    let Some(path) = recording_to() else {
        return;
    };
    if sightings.is_empty() {
        return;
    }
    let mut log = GapLog::load(&path);
    let today = today();
    for (sighting, items) in sightings {
        log.note(sighting, *items, &today, crate::CARREL_VERSION);
    }
    let _ = log.save(&path);
}

/// Records the files in `run` whose text couldn't be read, each type and reason as a gap.
pub fn from_run(run: &crate::ingest::run::Run) {
    if recording_to().is_none() {
        return;
    }
    let mut sightings: Vec<(Sighting, u64)> = Vec::new();
    for (candidate, text) in run.candidates.iter().zip(&run.texts) {
        let Some(text) = text else { continue };
        let (Some(why), Some(media_type)) = (text.unread, &text.media_type) else {
            continue;
        };
        if candidate.kind != crate::card::Kind::File {
            continue;
        }
        let extension = crate::formats::extension(&candidate.path);
        let Some(sighting) = unread_sighting(media_type, &extension, why) else {
            continue;
        };
        match sightings.iter_mut().find(|(s, _)| *s == sighting) {
            Some((_, n)) => *n += 1,
            None => sightings.push((sighting, 1)),
        }
    }
    record(&sightings);
}

/// The stage name for drafting that failed with `error`, or `None` when the failure isn't
/// something Carrel lacks, such as a file that was moved away before it was drafted.
#[must_use]
pub fn drafting_stage(error: &crate::ingest::IngestError) -> Option<&'static str> {
    use crate::ingest::IngestError;
    match error {
        IngestError::Missing(_) | IngestError::InsideArchive(_) | IngestError::NeedsChoice(_) => {
            None
        }
        IngestError::Io { .. } => Some("drafting: reading the file"),
        IngestError::Archive(_) => Some("drafting: reading the archive"),
        IngestError::Manifest(_) => Some("drafting: building the folder's manifest"),
    }
}

/// Records that drafting the item at `path` failed for good with `error`, unless the failure
/// isn't something Carrel lacks.
pub fn drafting_failed(path: &Path, error: &crate::ingest::IngestError) {
    let Some(stage) = drafting_stage(error) else {
        return;
    };
    if recording_to().is_none() {
        return;
    }
    let (media_type, extension) = type_of(path);
    record(&[(
        Sighting::new(GapKind::DraftingFailed, &media_type, &extension, stage),
        1,
    )]);
}

/// Records that the crawler couldn't judge the item at `path` with the model `model`
/// (`provider/offer`), because `why`: a model that failed, or none to judge with.
pub fn crawler_failed(path: Option<&Path>, model: &str, why: CrawlerGap) {
    if recording_to().is_none() {
        return;
    }
    let (media_type, extension) = path.map(type_of).unwrap_or_default();
    let (kind, stage) = match why {
        CrawlerGap::ModelFailed => (GapKind::CrawlerFailed, format!("crawler: {model} failed")),
        CrawlerGap::Unusable => (
            GapKind::CrawlerFailed,
            format!("crawler: {model}'s answer wasn't usable"),
        ),
        CrawlerGap::NoModel => (GapKind::NoProvider, format!("crawler: {model} missing")),
    };
    record(&[(Sighting::new(kind, &media_type, &extension, &stage), 1)]);
}

/// Why the crawler couldn't judge an item, for [`crawler_failed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrawlerGap {
    /// Its model failed.
    ModelFailed,
    /// Its model's answer wasn't usable.
    Unusable,
    /// Its model isn't on this device any more.
    NoModel,
}

/// Records that the librarian, working a flag on an item of `media_type` and `extension`,
/// concluded that fixing the card needs something Carrel can't do. `capability` names it in
/// a few words Carrel chooses, such as `text from 3D print files`, never the model's own
/// words, which could quote the item. For #454.
pub fn librarian_cant_help(media_type: &str, extension: &str, capability: &str) {
    record(&[(
        Sighting::new(
            GapKind::LibrarianCantHelp,
            media_type,
            extension,
            &format!("librarian: {capability}"),
        ),
        1,
    )]);
}

/// The media type and extension of the item at `path`: `inode/directory` for a folder.
#[must_use]
pub fn type_of(path: &Path) -> (String, String) {
    if path.is_dir() {
        return ("inode/directory".into(), String::new());
    }
    (
        crate::ingest::text::media_type(path),
        crate::formats::extension(path),
    )
}

/// This device's gaps from its configuration folder `config`, with the types
/// `unreadable.json` couldn't read brought in and saved, so each has its number.
#[must_use]
pub fn list_in(config: &Path) -> GapLog {
    let path = file_in(config);
    let mut log = GapLog::load(&path);
    let unreadable = Unreadable::load(&config.join("unreadable.json"));
    if log.bring_in_unreadable(&unreadable, crate::CARREL_VERSION) {
        let _ = log.save(&path);
    }
    log
}

/// Records that gap `number` in the configuration folder `config` was reported at
/// `address`, or clears it for an empty address. The gap as it is now.
///
/// # Errors
///
/// [`GapError`] if there's no such gap, the address isn't one, or the log can't be saved.
pub fn mark_reported_in(config: &Path, number: u32, address: &str) -> Result<Gap, GapError> {
    let mut log = list_in(config);
    log.mark_reported(number, address)?;
    log.save(&file_in(config))
        .map_err(|e| GapError::Io(e.to_string()))?;
    log.get(number).cloned().ok_or(GapError::NoGap(number))
}

/// Records a missing feature the owner reports in their own `words`, in the configuration
/// folder `config`, about the item `item` of `archive` when one is given: its type is
/// recorded, never its name. The gap.
///
/// # Errors
///
/// [`GapError`] if `words` are empty or the log can't be saved.
pub fn report_missing_feature_in(
    config: &Path,
    archive: Option<&crate::archive::Archive>,
    item: Option<&crate::archive::ItemRef>,
    words: &str,
) -> Result<Gap, GapError> {
    let (media_type, extension) = match (archive, item) {
        (Some(archive), Some(item)) => match &item.item {
            Some(path) => type_of(&archive.root().join(path)),
            None => ("text/markdown".into(), String::new()),
        },
        _ => (String::new(), String::new()),
    };
    let sighting = Sighting::new(GapKind::OwnerReported, &media_type, &extension, "");
    let mut log = list_in(config);
    let number = log.report_by_hand(&sighting, words, &today(), crate::CARREL_VERSION)?;
    log.save(&file_in(config))
        .map_err(|e| GapError::Io(e.to_string()))?;
    log.get(number).cloned().ok_or(GapError::NoGap(number))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stl() -> Sighting {
        Sighting::new(GapKind::UnreadFormat, "model/3mf", "3MF", EXTRACTION)
    }

    #[test]
    fn sightings_of_one_gap_add_to_one_entry_most_items_first() {
        let mut log = GapLog::default();
        let first = log.note(&stl(), 1, "2026-10-30", "0.6.0");
        let other = log.note(
            &Sighting::new(GapKind::CrawlerFailed, "", "", "crawler: llama/x failed"),
            1,
            "2026-10-30",
            "0.6.0",
        );
        assert_eq!(log.note(&stl(), 2, "2026-10-31", "0.6.1"), first);
        assert_ne!(first, other);
        let gap = &log.gaps[0];
        assert_eq!(
            (gap.number, gap.items, gap.extension.as_str()),
            (1, 3, "3mf")
        );
        assert_eq!(
            (gap.first_version.as_str(), gap.last_version.as_str()),
            ("0.6.0", "0.6.1")
        );
        assert_eq!(gap.last_seen, "2026-10-31");
        assert_eq!(log.gaps.len(), 2);
        assert_eq!(
            gap.summary(),
            "no reader for the file type: .3mf (model/3mf), at text extraction"
        );
    }

    #[test]
    fn numbers_are_never_reused() {
        let mut log = GapLog::default();
        log.note(&stl(), 1, "2026-10-30", "0.6.0");
        log.gaps.clear();
        let next = log.note(&stl(), 1, "2026-10-30", "0.6.0");
        assert_eq!(next, 2);
    }

    #[test]
    fn the_owner_reports_in_their_own_words() {
        let mut log = GapLog::default();
        assert_eq!(
            log.report_by_hand(&stl(), "  ", "2026-10-30", "0.6.0"),
            Err(GapError::NoWords)
        );
        let n = log
            .report_by_hand(&stl(), "Read the plate names", "2026-10-30", "0.6.0")
            .unwrap();
        let gap = log.get(n).unwrap();
        assert_eq!(gap.kind, GapKind::OwnerReported);
        assert_eq!(gap.words.as_deref(), Some("Read the plate names"));
        let again = log
            .report_by_hand(&stl(), "Something else", "2026-10-30", "0.6.0")
            .unwrap();
        assert_ne!(n, again, "different words are a different gap");
    }

    #[test]
    fn a_reported_gap_keeps_the_issue_address() {
        let mut log = GapLog::default();
        let n = log.note(&stl(), 1, "2026-10-30", "0.6.0");
        assert!(matches!(
            log.mark_reported(n, "issue 12"),
            Err(GapError::NotAnAddress(_))
        ));
        assert_eq!(
            log.mark_reported(9, "https://github.com/x/y/issues/1"),
            Err(GapError::NoGap(9))
        );
        log.mark_reported(n, "https://github.com/Horizon-Labs/carrel/issues/500")
            .unwrap();
        assert_eq!(
            log.get(n).unwrap().reported.as_deref(),
            Some("https://github.com/Horizon-Labs/carrel/issues/500")
        );
        log.mark_reported(n, "").unwrap();
        assert_eq!(log.get(n).unwrap().reported, None);
    }

    #[test]
    fn unreadable_types_become_gaps_without_counting_twice() {
        let mut unreadable = Unreadable::default();
        unreadable.note("model/3mf", "3mf", Unread::NoReader, "2026-10-01");
        unreadable.note("model/3mf", "3mf", Unread::NoReader, "2026-10-02");
        unreadable.note("image/png", "png", Unread::Empty, "2026-10-02");
        let mut log = GapLog::default();
        assert!(log.bring_in_unreadable(&unreadable, "0.6.0"));
        assert_eq!(log.gaps.len(), 1, "empty text isn't a gap");
        let gap = &log.gaps[0];
        assert_eq!((gap.items, gap.first_seen.as_str()), (2, "2026-10-01"));
        assert!(!log.bring_in_unreadable(&unreadable, "0.6.0"));
        log.note(&stl(), 1, "2026-10-03", "0.6.0");
        assert!(!log.bring_in_unreadable(&unreadable, "0.6.0"));
        assert_eq!(log.gaps[0].items, 3);
    }

    #[test]
    fn a_newer_carrel_may_have_addressed_a_gap() {
        let mut log = GapLog::default();
        let n = log.note(&stl(), 1, "2026-10-30", "0.6.0");
        let gap = log.get(n).unwrap();
        assert!(gap.not_seen_since_update("0.7.0"));
        assert!(gap.not_seen_since_update("0.6.1"));
        assert!(!gap.not_seen_since_update("0.6.0"));
        assert!(!gap.not_seen_since_update("0.5.9"));
    }

    #[test]
    fn the_log_is_saved_and_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep").join(FILE);
        let mut log = GapLog::default();
        log.note(&stl(), 4, "2026-10-30", "0.6.0");
        log.save(&path).unwrap();
        assert_eq!(GapLog::load(&path), log);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"kind\": \"unread-format\""), "{text}");
        assert_eq!(
            GapLog::load(&dir.path().join("none.json")),
            GapLog::default()
        );
    }

    #[test]
    fn drafting_failures_that_carrel_doesnt_lack_are_left_out() {
        use crate::ingest::IngestError;
        assert_eq!(drafting_stage(&IngestError::Missing("/x".into())), None);
        assert!(
            drafting_stage(&IngestError::Io {
                path: "/x".into(),
                source: std::io::Error::other("no"),
            })
            .is_some()
        );
    }

    #[test]
    fn kinds_are_named_as_json_names_them() {
        for kind in [
            GapKind::UnreadFormat,
            GapKind::ExtractionFailed,
            GapKind::DraftingFailed,
            GapKind::CrawlerFailed,
            GapKind::NoProvider,
            GapKind::LibrarianCantHelp,
            GapKind::OwnerReported,
        ] {
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::from(kind.name())
            );
        }
    }
}
