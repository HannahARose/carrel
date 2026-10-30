//! Stage 12: filing approved proposals into the archive (`docs/design/ingest.md`). Each item is
//! moved (or, with `ingest.originals: keep`, copied) into its collection's folder, checked
//! against the checksum it was proposed with, and given its card. The whole batch is then one
//! history commit, and the filed proposals leave the queue.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::archive::{Archive, ArchiveError, Collection, escape_link_text};
use crate::card::{Card, CardMarker, Checksum, DateOrTime, Extra, Id, Kind, ManifestRef};
use crate::card_file::{CardFile, CardFileError};
use crate::coordinator::{CoordinatorError, DEFAULT_LEASE, WriterLease};
use crate::documents::{CollectionDoc, Originals, TagDef};
use crate::history::{Change, History, HistoryError, Operation};
use crate::manifest::{Manifest, ManifestError};
use crate::review::{Problem, ReviewEntry, ReviewError, ReviewQueue, State};

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
    /// Where the original was, when it was left there (`ingest.originals: keep`).
    pub kept_original: Option<PathBuf>,
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
}

impl FilingError {
    /// Whether the proposal should wait for the owner because of it, rather than for another
    /// device or a retry.
    #[must_use]
    pub fn needs_owner(&self) -> bool {
        matches!(
            self,
            Self::Missing(_) | Self::Changed(_) | Self::NoCollection(_) | Self::Card(_)
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
/// the queue. Everything filed is one history commit, with operation `ingest`.
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
    outcome.commit = history.commit(archive, &change, lease)?;
    Ok(outcome)
}

/// Files one proposal: places the item, writes its manifest and card, adds an approved new
/// tag to the collection's vocabulary and a line to its changelog.
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

    let id = Id::new_v7();
    let original_name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| FilingError::Missing(source.clone()))?;
    let folder = archive.root().join(&collection.path);
    let name = free_name(&folder, &original_name).map_err(io(&folder))?;
    let mut card = card_for(entry, id, &name, placement, collection);
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
    place(
        source,
        &target,
        &entry.source.checksum,
        entry.kind,
        &rules,
        originals,
    )?;
    if entry.kind == Kind::File {
        card.checksum = Some(entry.source.checksum.clone());
        card.size = Some(entry.source.size);
    } else {
        let manifest = Manifest::build(&target, id, &rules, None)?;
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
    let note = if name == original_name {
        String::new()
    } else {
        format!(
            "\nFiled as “{name}”: the collection already had an item named “{original_name}”.\n"
        )
    };
    let card_name = format!("{name}.md");
    let text = CardFile::new(card, &note)?.to_text()?;
    write_new(&folder.join(&card_name), &text)?;

    if placement == Placement::Destination {
        add_tags(archive, collection, entry)?;
    }
    add_to_changelog(archive, collection, &entry.card.title, &card_name)?;
    Ok(Filed {
        entry: entry.id,
        item: id,
        title: entry.card.title.clone(),
        path: format!("{}/{name}", collection.path),
        card: format!("{}/{card_name}", collection.path),
        kept_original: (originals == Originals::Keep).then(|| source.clone()),
    })
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
        links: Vec::new(),
        dates: Some(dates),
        sensitivity: draft.sensitivity,
        checksum: None,
        size: None,
        manifest: None,
        location: None,
        url: None,
        checked_out: None,
        generated: draft.generated.clone(),
        removed: None,
        extra: draft.extra.clone(),
    }
}

/// `name`, or `name (2)`, `name (3)`… before any extension, whichever is free in `folder`
/// along with its card name. Names are compared without case (FORMAT.md §2.5).
fn free_name(folder: &Path, name: &str) -> std::io::Result<String> {
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

fn copy(source: &Path, target: &Path) -> Result<(), FilingError> {
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
    let path = archive.root().join(&collection.path).join("CHANGELOG.md");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "# Changelog\n".into(),
        Err(e) => return Err(io(&path)(e)),
    };
    let line = format!(
        "- {} · added [{}]({}) from ingest\n",
        jiff::Zoned::now().strftime("%Y-%m-%d"),
        escape_link_text(title),
        link_destination(card_name)
    );
    // After the heading and its blank line, before the newest entry.
    let at = text.find("\n- ").map_or(text.len(), |i| i + 1);
    let mut updated = text[..at].to_owned();
    if !updated.ends_with("\n\n") {
        if !updated.ends_with('\n') {
            updated.push('\n');
        }
        updated.push('\n');
    }
    updated.push_str(&line);
    updated.push_str(&text[at..]);
    replace(&path, &updated)
}

/// A path percent-encoded for a Markdown link destination (FORMAT.md §2.5).
fn link_destination(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            ' ' => out.push_str("%20"),
            '(' => out.push_str("%28"),
            ')' => out.push_str("%29"),
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            '<' => out.push_str("%3C"),
            '>' => out.push_str("%3E"),
            _ => out.push(ch),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_destinations_are_encoded() {
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
