//! Stage 12: filing approved proposals into the archive (`docs/design/ingest.md`). Each item is
//! moved (or, with `ingest.originals: keep`, copied) into its collection's folder, checked
//! against the checksum it was proposed with, and given its card. The whole batch is then one
//! history commit, and the filed proposals leave the queue.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::archive::{Archive, ArchiveError, Collection, escape_link_text};
use crate::card::{
    Card, CardMarker, Checksum, DateOrTime, ExpandedFrom, Extra, Id, Kind, ManifestRef, PlacedBy,
    PlacementConfidence, PlacementRecord,
};
use crate::card_file::{CardFile, CardFileError};
use crate::compressed::{self, plan::Choice};
use crate::coordinator::{CoordinatorError, DEFAULT_LEASE, WriterLease};
use crate::documents::{CollectionDoc, Originals, TagDef};
use crate::history::{Change, History, HistoryError, Operation};
use crate::manifest::{Manifest, ManifestError};
use crate::review::{Confidence, Problem, ReviewEntry, ReviewError, ReviewQueue, State};

/// Holding's folder (FORMAT.md §3).
const HOLDING: &str = "Holding";

/// Names a collection reserves (FORMAT.md §2.7), compared without case.
const RESERVED: [&str; 2] = ["_collection.md", "changelog.md"];

/// An item filed into the archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filed {
    /// The proposal it came from.
    pub entry: Id,
    /// The new item's identifier.
    pub item: Id,
    /// Its title.
    pub title: String,
    /// The item's path, relative to the archive root.
    pub path: String,
    /// Its card's path, relative to the archive root.
    pub card: String,
    /// Where the original was, when it was left there (`ingest.originals: keep`, or a
    /// compressed file the Trash couldn't take).
    pub kept_original: Option<PathBuf>,
    /// The compressed file it was expanded from.
    pub expanded_from: Option<PathBuf>,
    /// Why the expanded compressed file couldn't go to the Trash, when it couldn't.
    pub trash_problem: Option<String>,
}

/// Why one item couldn't be filed. The batch goes on without it.
#[derive(Debug)]
pub enum FilingError {
    /// The item is on another device; it waits for that device.
    OtherDevice(Id),
    /// The item is no longer where it was proposed from.
    Missing(PathBuf),
    /// The item changed after it was proposed, so the proposal may not fit it.
    Changed(PathBuf),
    /// The destination collection no longer exists.
    NoCollection(String),
    /// The card the proposal would write isn't valid.
    Card(Vec<String>),
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// Building a folder's manifest failed.
    Manifest(ManifestError),
    /// Reading the archive failed.
    Archive(ArchiveError),
    /// The writer lease was lost, so nothing more may be written.
    Lease(CoordinatorError),
    /// The compressed file is waiting for the owner to say whether to expand it.
    Undecided(String),
    /// The compressed file couldn't be expanded; it's left where it was.
    Expand(String),
}

impl FilingError {
    /// Whether the proposal should wait for the owner because of it, rather than for another
    /// device or a retry.
    #[must_use]
    pub fn needs_owner(&self) -> bool {
        matches!(
            self,
            Self::Missing(_)
                | Self::Changed(_)
                | Self::NoCollection(_)
                | Self::Card(_)
                | Self::Undecided(_)
                | Self::Expand(_)
        )
    }
}

impl std::fmt::Display for FilingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OtherDevice(device) => {
                write!(f, "the item is on device {device}; it can be filed there")
            }
            Self::Missing(path) => write!(f, "{} is no longer there", path.display()),
            Self::Changed(path) => write!(
                f,
                "{} changed after it was proposed; propose it again",
                path.display()
            ),
            Self::NoCollection(path) => write!(f, "the collection {path} no longer exists"),
            Self::Card(problems) => write!(f, "the card isn't valid: {}", problems.join("; ")),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Manifest(e) => write!(f, "{e}"),
            Self::Archive(e) => write!(f, "{e}"),
            Self::Lease(e) => write!(f, "{e}"),
            Self::Undecided(reason) => write!(
                f,
                "{reason}: say whether to expand it or keep it compressed"
            ),
            Self::Expand(reason) => write!(
                f,
                "it couldn't be expanded, so it was left where it was: {reason}"
            ),
        }
    }
}

impl std::error::Error for FilingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Manifest(e) => Some(e),
            Self::Archive(e) => Some(e),
            Self::Lease(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ManifestError> for FilingError {
    fn from(e: ManifestError) -> Self {
        Self::Manifest(e)
    }
}

impl From<ArchiveError> for FilingError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

impl From<CardFileError> for FilingError {
    fn from(e: CardFileError) -> Self {
        Self::Card(vec![e.to_string()])
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> FilingError + '_ {
    move |source| FilingError::Io {
        path: path.to_owned(),
        source,
    }
}

/// Where a decided proposal goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Its proposed destination.
    Destination,
    /// Holding, for the owner to place later.
    Holding,
}

/// Whether a new proposal is filed without review: only when an owner rule with
/// `auto_file: true` chose its destination, and never for sensitive items, duplicates or
/// proposals that need the owner (design decision 4).
#[must_use]
pub fn files_itself(entry: &ReviewEntry, route: &crate::ingest::route::Route) -> bool {
    route.auto_file
        && route.rule.is_some()
        && entry.state == State::Proposed
        && entry.card.sensitivity < crate::card::Sensitivity::Sensitive
        && entry.duplicate_of.is_none()
}

/// What filing a batch did.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The items filed, in order.
    pub filed: Vec<Filed>,
    /// Proposals that couldn't be filed, and why. Those that need the owner are saved as
    /// `needs-owner` with the reason as a problem; the rest stay approved or held.
    pub failed: Vec<(ReviewEntry, FilingError)>,
    /// The history commit, if anything was filed.
    pub commit: Option<String>,
    /// The snapshot taken with the commit, when backups are set up and reachable.
    pub snapshot: Option<crate::backup::Snapshot>,
    /// Why no snapshot was taken, when backups are set up but failed.
    pub snapshot_error: Option<crate::backup::BackupError>,
    /// Listings that couldn't be brought up to date (FORMAT.md §6.3), and why.
    pub listing_problems: Vec<String>,
}

/// Why a batch couldn't be filed at all.
#[derive(Debug)]
pub enum BatchError {
    /// The review queue couldn't be written.
    Review(ReviewError),
    /// The history couldn't be committed.
    History(HistoryError),
    /// The archive couldn't be read.
    Archive(ArchiveError),
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Review(e) => write!(f, "{e}"),
            Self::History(e) => write!(f, "{e}"),
            Self::Archive(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Review(e) => Some(e),
            Self::History(e) => Some(e),
            Self::Archive(e) => Some(e),
        }
    }
}

impl From<ReviewError> for BatchError {
    fn from(e: ReviewError) -> Self {
        Self::Review(e)
    }
}

impl From<HistoryError> for BatchError {
    fn from(e: HistoryError) -> Self {
        Self::History(e)
    }
}

impl From<ArchiveError> for BatchError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

/// Files decided proposals as one batch, holding `lease`: approved ones go to their
/// destination, held ones to Holding. Items on other devices, and items that fail, are left in
/// the queue. Everything filed is one history commit, with operation `ingest`. With `backups`,
/// the archive is snapshotted first and the commit names the snapshot; a failed snapshot
/// doesn't stop the commit (`docs/design/backups.md`).
///
/// # Errors
///
/// [`BatchError`] if the queue or history can't be written. Items filed before that stay
/// filed; the next commit records them.
pub fn file_batch(
    archive: &Archive,
    decided: Vec<(ReviewEntry, Placement)>,
    lease: &mut WriterLease,
    device: Id,
    backups: Option<&dyn crate::backup::Backups>,
) -> Result<Outcome, BatchError> {
    let history = History::open(archive)?;
    let queue = ReviewQueue::of(archive);
    let mut outcome = Outcome::default();
    for (mut entry, placement) in decided {
        entry.state = match placement {
            Placement::Destination => State::Approved,
            Placement::Holding => State::Held,
        };
        // The collections are read again for each item, since filing can add a tag.
        let collections = archive.collections()?;
        let result = lease
            .renew(DEFAULT_LEASE)
            .map_err(FilingError::Lease)
            .and_then(|()| file_one(archive, &collections, &entry, placement, device));
        match result {
            Ok(filed) => {
                queue.remove(entry.id)?;
                outcome.filed.push(filed);
            }
            Err(error) => {
                if error.needs_owner() {
                    entry.state = State::NeedsOwner;
                    entry.problems.push(Problem {
                        stage: "archive".into(),
                        message: error.to_string(),
                    });
                }
                let lost = matches!(error, FilingError::Lease(_));
                queue.save(&entry)?;
                outcome.failed.push((entry, error));
                if lost {
                    break;
                }
            }
        }
    }
    if outcome.filed.is_empty() {
        return Ok(outcome);
    }
    let count = outcome.filed.len();
    let mut change = Change::new(
        Operation::Ingest,
        if count == 1 {
            format!("File “{}”", outcome.filed[0].title)
        } else {
            format!("File {count} items")
        },
        lease.info().person,
        device,
    );
    let mut body = String::new();
    for filed in &outcome.filed {
        writeln!(body, "- {} → {}", filed.title, filed.path).unwrap();
    }
    change.body = Some(body);
    change.items = outcome.filed.iter().map(|f| f.item).collect();
    match crate::listing::refresh(archive) {
        Ok(refreshed) => outcome.listing_problems = refreshed.problems,
        Err(error) => outcome.listing_problems.push(error.to_string()),
    }
    if let Some(backups) = backups {
        match backups.snapshot(archive.root(), &change.summary) {
            Ok(snapshot) => {
                change.snapshot = Some(snapshot.trailer());
                outcome.snapshot = Some(snapshot);
            }
            Err(error) => outcome.snapshot_error = Some(error),
        }
    }
    outcome.commit = history.commit(archive, &change, lease)?;
    Ok(outcome)
}

/// Files one proposal: places the item, writes its manifest and card, adds an approved new
/// tag to the collection's vocabulary and a line to its changelog. A compressed file the
/// proposal plans to expand is expanded first, into a temporary folder in the archive, and
/// filed as what it held; once that's in place, the compressed file goes to the system Trash
/// (`docs/design/compressed.md`).
///
/// # Errors
///
/// [`FilingError`]; nothing in the archive has changed unless the item was placed.
pub fn file_one(
    archive: &Archive,
    collections: &[Collection],
    entry: &ReviewEntry,
    placement: Placement,
    device: Id,
) -> Result<Filed, FilingError> {
    if entry.source.device != device {
        return Err(FilingError::OtherDevice(entry.source.device));
    }
    let source = &entry.source.path;
    if std::fs::symlink_metadata(source).is_err() {
        return Err(FilingError::Missing(source.clone()));
    }
    if let Some(plan) = entry
        .compressed
        .as_ref()
        .filter(|p| p.action == Choice::Ask)
    {
        return Err(FilingError::Undecided(
            plan.reason
                .clone()
                .unwrap_or_else(|| "the owner decides".into()),
        ));
    }
    let rules = archive
        .rules()
        .map_err(|e| FilingError::Card(vec![e.to_string()]))?;
    if fingerprint(source, entry.kind, &rules)? != entry.source.checksum {
        return Err(FilingError::Changed(source.clone()));
    }
    let collection = match placement {
        Placement::Destination => collections
            .iter()
            .find(|c| c.doc.id == entry.destination.collection),
        Placement::Holding => collections.iter().find(|c| c.path == HOLDING),
    }
    .ok_or_else(|| {
        FilingError::NoCollection(match placement {
            Placement::Destination => entry.destination.path.clone(),
            Placement::Holding => HOLDING.into(),
        })
    })?;
    let expansion = expand(archive, entry, &rules)?;
    let filed = file_item(
        archive,
        entry,
        placement,
        collection,
        &rules,
        expansion.as_ref(),
    );
    if let Some(expansion) = &expansion {
        let _ = remove(&expansion.temp);
    }
    filed
}

/// A compressed file expanded for filing.
struct Expansion {
    /// The temporary folder it was expanded into, removed afterwards.
    temp: PathBuf,
    /// What it held, inside `temp`.
    item: PathBuf,
    /// What that is.
    kind: Kind,
    /// For a bundle, how it's sealed.
    sealed: Option<crate::card::Sealing>,
}

/// Expands `entry`'s compressed file when its plan says to, into a temporary folder on the
/// archive's volume; `None` when there's nothing to expand.
fn expand(
    archive: &Archive,
    entry: &ReviewEntry,
    rules: &crate::detect::Rules,
) -> Result<Option<Expansion>, FilingError> {
    let Some(plan) = entry
        .compressed
        .as_ref()
        .filter(|p| p.action == Choice::Expand)
    else {
        return Ok(None);
    };
    let temp = archive
        .root()
        .join("_System/cache.nosync/expanding")
        .join(Id::new_v7().to_string());
    std::fs::create_dir_all(&temp).map_err(io(&temp))?;
    let limits = (!plan.by_owner).then_some(&compressed::Limits::DEFAULT);
    let expanded = match compressed::expand(&entry.source.path, plan.format, &temp, limits) {
        Ok(expanded) => expanded,
        Err(error) => {
            let _ = remove(&temp);
            return Err(FilingError::Expand(error.to_string()));
        }
    };
    let detected = rules.detect(&expanded.path).map_err(io(&expanded.path))?;
    Ok(Some(Expansion {
        temp,
        item: expanded.path,
        kind: detected.kind,
        sealed: detected.sealed,
    }))
}

/// Places the item (or what an expanded compressed file held) and writes its card.
fn file_item(
    archive: &Archive,
    entry: &ReviewEntry,
    placement: Placement,
    collection: &Collection,
    rules: &crate::detect::Rules,
    expansion: Option<&Expansion>,
) -> Result<Filed, FilingError> {
    let source = &entry.source.path;
    let id = Id::new_v7();
    let original_name = expansion
        .map_or(source.as_path(), |e| e.item.as_path())
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| FilingError::Missing(source.clone()))?;
    let folder = archive.root().join(&collection.path);
    // Names Carrel creates are always portable (FORMAT.md §2.5).
    let portable = crate::names::portable(&original_name);
    let name = free_name(&folder, &portable).map_err(io(&folder))?;
    let mut card = card_for(entry, id, &name, placement, collection);
    let kind = expansion.map_or(entry.kind, |e| e.kind);
    if let Some(expansion) = expansion {
        card.kind = expansion.kind;
        card.sealed = expansion.sealed;
        card.expanded_from = Some(ExpandedFrom {
            name: source
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: entry.source.size,
            checksum: entry.source.checksum.clone(),
            extra: Extra::new(),
        });
    }
    let problems: Vec<String> = card
        .problems()
        .into_iter()
        .filter(|p| !matches!(p.field.as_str(), "checksum" | "size" | "manifest"))
        .map(|p| p.to_string())
        .collect();
    if !problems.is_empty() {
        return Err(FilingError::Card(problems));
    }

    let target = folder.join(&name);
    let originals = archive.settings().ingest.originals;
    if let Some(expansion) = expansion {
        std::fs::create_dir_all(&folder).map_err(io(&folder))?;
        std::fs::rename(&expansion.item, &target).map_err(io(&target))?;
    } else {
        place(
            source,
            &target,
            &entry.source.checksum,
            entry.kind,
            rules,
            originals,
        )?;
    }
    if expansion.is_none() && kind == Kind::File {
        card.checksum = Some(entry.source.checksum.clone());
        card.size = Some(entry.source.size);
    } else {
        describe_contents(archive, &mut card, &target, id, kind, rules)?;
    }
    let note = name_note(&name, &portable, &original_name);
    let card_name = format!("{name}.md");
    let text = CardFile::new(card, &note)?.to_text()?;
    write_new(&folder.join(&card_name), &text)?;

    if placement == Placement::Destination {
        add_tags(archive, collection, entry)?;
    }
    add_to_changelog(archive, collection, &entry.card.title, &card_name)?;
    // The compressed file goes to the Trash only once what it held is filed, and stays where
    // it was if the Trash can't take it.
    let mut trash_problem = None;
    if expansion.is_some()
        && originals == Originals::Move
        && let Err(error) = crate::trash::put(source)
    {
        trash_problem = Some(error.to_string());
    }
    Ok(Filed {
        entry: entry.id,
        item: id,
        title: entry.card.title.clone(),
        path: format!("{}/{name}", collection.path),
        card: format!("{}/{card_name}", collection.path),
        kept_original: (originals == Originals::Keep || trash_problem.is_some())
            .then(|| source.clone()),
        expanded_from: expansion.map(|_| source.clone()),
        trash_problem,
    })
}

/// Fills in the card fields that describe the placed item's contents: a file's checksum and
/// size, or a folder's manifest.
fn describe_contents(
    archive: &Archive,
    card: &mut Card,
    target: &Path,
    id: Id,
    kind: Kind,
    rules: &crate::detect::Rules,
) -> Result<(), FilingError> {
    if kind == Kind::File {
        card.checksum = Some(Checksum::of_file(target).map_err(io(target))?);
        card.size = Some(std::fs::metadata(target).map_err(io(target))?.len());
    } else {
        let manifest = Manifest::build(target, id, rules, None)?;
        let path = format!("_System/manifests/{id}.tsv");
        let file = archive.root().join(&path);
        write_new(&file, &manifest.to_tsv())?;
        card.size = Some(manifest.total_size());
        card.manifest = Some(ManifestRef {
            checksum: manifest.checksum(),
            path,
            extra: Extra::new(),
        });
    }
    Ok(())
}

/// The note a card gets when the item was filed under another name.
fn name_note(name: &str, portable: &str, original_name: &str) -> String {
    if name == original_name {
        String::new()
    } else if portable != original_name {
        format!(
            "\nFiled as “{name}”: “{original_name}” {}, so it wouldn't work on every system.\n",
            crate::names::problems(original_name).join(" and ")
        )
    } else {
        format!(
            "\nFiled as “{name}”: the collection already had an item named “{original_name}”.\n"
        )
    }
}

/// A file's checksum, or a folder's manifest content checksum.
fn fingerprint(
    path: &Path,
    kind: Kind,
    rules: &crate::detect::Rules,
) -> Result<Checksum, FilingError> {
    if kind == Kind::File {
        Checksum::of_file(path).map_err(io(path))
    } else {
        Ok(Manifest::build(path, Id::new_v7(), rules, None)?.checksum())
    }
}

/// What a filed card keeps of routing's uncertainty (FORMAT.md §5.8): nothing for a certain
/// placement, or for Holding, where being there says the same.
fn placement_record(entry: &ReviewEntry, placement: Placement) -> Option<PlacementRecord> {
    let confidence = match (placement, entry.destination.confidence) {
        (Placement::Holding, _) | (_, Confidence::High) => return None,
        (Placement::Destination, Confidence::Medium) => PlacementConfidence::Medium,
        (Placement::Destination, Confidence::Low) => PlacementConfidence::Low,
    };
    Some(PlacementRecord {
        confidence,
        by: if entry.destination.rule.is_some() {
            PlacedBy::Rule
        } else {
            PlacedBy::Model
        },
        at: DateOrTime(
            jiff::Zoned::now()
                .strftime("%Y-%m-%dT%H:%M:%S%:z")
                .to_string(),
        ),
        extra: Extra::new(),
    })
}

/// The card for a filed proposal, without the fields that depend on where it lands.
fn card_for(
    entry: &ReviewEntry,
    id: Id,
    name: &str,
    placement: Placement,
    collection: &Collection,
) -> Card {
    let draft = &entry.card;
    let mut dates = draft.dates.clone().unwrap_or_default();
    dates.added = Some(DateOrTime(
        jiff::Zoned::now().strftime("%Y-%m-%d").to_string(),
    ));
    // Tags only apply where they're in the vocabulary; Holding has its own.
    let tags = match placement {
        Placement::Destination => {
            let mut tags = draft.tags.clone();
            for tag in &entry.proposed_tags {
                if !tags.contains(tag) {
                    tags.push(tag.clone());
                }
            }
            tags
        }
        Placement::Holding => draft
            .tags
            .iter()
            .filter(|t| collection.doc.tags.iter().any(|d| d.name == **t))
            .cloned()
            .collect(),
    };
    Card {
        carrel: CardMarker::Card,
        format: crate::FORMAT_VERSION,
        id,
        kind: entry.kind,
        title: draft.title.clone(),
        item: Some(name.to_owned()),
        sealed: entry.sealed,
        allow_readme: None,
        summary: draft.summary.clone(),
        tags,
        links: draft
            .links
            .iter()
            .map(|link| crate::card::Link {
                path: link
                    .path
                    .as_deref()
                    .map(|to| crate::listing::relative(&collection.path, to)),
                ..link.clone()
            })
            .collect(),
        dates: Some(dates),
        sensitivity: draft.sensitivity,
        checksum: None,
        size: None,
        manifest: None,
        location: None,
        url: None,
        checked_out: None,
        snapshots: None,
        generated: draft.generated.clone(),
        placement: placement_record(entry, placement),
        removed: None,
        expanded_from: None,
        extra: draft.extra.clone(),
    }
}

/// `name`, or `name (2)`, `name (3)`… before any extension, whichever is free in `folder`
/// along with its card name. Names are compared without case (FORMAT.md §2.5).
pub(crate) fn free_name(folder: &Path, name: &str) -> std::io::Result<String> {
    let taken: Vec<String> = match std::fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().to_lowercase())
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e),
    };
    let is_free = |candidate: &str| {
        let lower = candidate.to_lowercase();
        let card = format!("{lower}.md");
        ![lower.as_str(), card.as_str()]
            .iter()
            .any(|n| taken.iter().any(|t| t == n) || RESERVED.contains(n))
    };
    if is_free(name) {
        return Ok(name.to_owned());
    }
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    let mut n = 2;
    loop {
        let candidate = format!("{stem} ({n}){extension}");
        if is_free(&candidate) {
            return Ok(candidate);
        }
        n += 1;
    }
}

/// Moves or copies `source` to `target` and checks the result against `checksum`. A move
/// within one volume is a rename; across volumes it's a copy, a check, then removing the
/// original.
fn place(
    source: &Path,
    target: &Path,
    checksum: &Checksum,
    kind: Kind,
    rules: &crate::detect::Rules,
    originals: Originals,
) -> Result<(), FilingError> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    if originals == Originals::Move {
        match std::fs::rename(source, target) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {}
            Err(e) => return Err(io(source)(e)),
        }
    }
    copy(source, target)?;
    if fingerprint(target, kind, rules)? != *checksum {
        let _ = remove(target);
        return Err(FilingError::Changed(source.to_owned()));
    }
    if originals == Originals::Move {
        remove(source).map_err(io(source))?;
    }
    Ok(())
}

pub(crate) fn copy(source: &Path, target: &Path) -> Result<(), FilingError> {
    let meta = std::fs::symlink_metadata(source).map_err(io(source))?;
    if meta.file_type().is_symlink() {
        let link = std::fs::read_link(source).map_err(io(source))?;
        symlink(&link, target, source.is_dir()).map_err(io(target))
    } else if meta.is_dir() {
        std::fs::create_dir(target).map_err(io(target))?;
        for child in std::fs::read_dir(source).map_err(io(source))? {
            let child = child.map_err(io(source))?;
            copy(&child.path(), &target.join(child.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(source, target).map_err(io(target))?;
        Ok(())
    }
}

#[cfg(unix)]
fn symlink(link: &Path, at: &Path, _dir: bool) -> std::io::Result<()> {
    std::os::unix::fs::symlink(link, at)
}

#[cfg(windows)]
fn symlink(link: &Path, at: &Path, dir: bool) -> std::io::Result<()> {
    if dir {
        std::os::windows::fs::symlink_dir(link, at)
    } else {
        std::os::windows::fs::symlink_file(link, at)
    }
}

fn remove(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}

/// Writes a file that must not exist yet, creating its folder.
fn write_new(path: &Path, contents: &str) -> Result<(), FilingError> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io(path))?;
    file.write_all(contents.as_bytes()).map_err(io(path))
}

/// Replaces a file through a temporary file beside it, so readers never see half of it.
fn replace(path: &Path, contents: &str) -> Result<(), FilingError> {
    let temp = path.with_extension(format!("tmp-{}", Id::new_v7()));
    std::fs::write(&temp, contents).map_err(io(&temp))?;
    std::fs::rename(&temp, path).map_err(io(path))
}

/// Adds the proposal's approved new tags to the collection's vocabulary.
fn add_tags(
    archive: &Archive,
    collection: &Collection,
    entry: &ReviewEntry,
) -> Result<(), FilingError> {
    let new: Vec<_> = entry
        .proposed_tags
        .iter()
        .filter(|t| !collection.doc.tags.iter().any(|d| d.name == **t))
        .collect();
    if new.is_empty() {
        return Ok(());
    }
    let path = archive.root().join(&collection.path).join("_collection.md");
    let text = std::fs::read_to_string(&path).map_err(io(&path))?;
    let invalid =
        |message: String| FilingError::Card(vec![format!("{}: {message}", path.display())]);
    let split = crate::frontmatter::split(&text).map_err(|e| invalid(e.to_string()))?;
    let mut doc: CollectionDoc =
        serde_saphyr::from_str(split.yaml).map_err(|e| invalid(e.to_string()))?;
    for tag in new {
        doc.tags.push(TagDef {
            name: tag.clone(),
            description: format!("Added on approving “{}”", entry.card.title),
            extra: Extra::new(),
        });
    }
    let yaml = serde_saphyr::to_string(&doc).map_err(|e| invalid(e.to_string()))?;
    replace(&path, &crate::frontmatter::join(&yaml, split.body))
}

/// Adds `- DATE · added [TITLE](CARD) from ingest` to the top of the collection's changelog.
fn add_to_changelog(
    archive: &Archive,
    collection: &Collection,
    title: &str,
    card_name: &str,
) -> Result<(), FilingError> {
    let entry = format!(
        "added [{}]({}) from ingest",
        escape_link_text(title),
        crate::listing::link_destination(card_name)
    );
    crate::listing::add_to_changelog(archive, &collection.path, &entry).map_err(io(&archive
        .root()
        .join(&collection.path)
        .join("CHANGELOG.md")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_destinations_are_encoded() {
        use crate::listing::link_destination;
        assert_eq!(
            link_destination("RFC 9562 (draft).md"),
            "RFC%209562%20%28draft%29.md"
        );
    }

    #[test]
    fn taken_names_get_a_number_before_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Notes.md"), "").unwrap();
        std::fs::write(dir.path().join("lamp.txt.md"), "").unwrap();
        assert_eq!(free_name(dir.path(), "a.txt").unwrap(), "a.txt");
        assert_eq!(free_name(dir.path(), "notes.md").unwrap(), "notes (2).md");
        assert_eq!(free_name(dir.path(), "lamp.txt").unwrap(), "lamp (2).txt");
        assert_eq!(
            free_name(dir.path(), "CHANGELOG.md").unwrap(),
            "CHANGELOG (2).md"
        );
        assert_eq!(free_name(dir.path(), "src").unwrap(), "src");
    }
}
