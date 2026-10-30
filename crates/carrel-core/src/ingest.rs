//! Ingest: putting new files and folders into the archive (`docs/design/ingest.md`).
//!
//! This module has the stages that need no model: intake (which paths become items), kind
//! (file, open folder or sealed bundle), fingerprint (checksum and size), exact duplicates,
//! and in [`text`] the item's text for the models. Nothing here changes the archive or the
//! items.

pub mod card;
pub mod filing;
pub mod questions;
pub mod route;
pub mod sensitivity;
pub mod text;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::archive::{Archive, ArchiveError};
use crate::card::{Checksum, Id, Kind, Sealing};
use crate::card_file::CardFile;
use crate::detect::is_ignored;
use crate::manifest::{Manifest, ManifestError};
use crate::review::{CardDraft, Destination, Problem, ReviewEntry, ReviewMarker, Source, State};

/// Folder names that usually hold a pile of unrelated things rather than one item, so ingest
/// asks instead of guessing.
pub const PILE_FOLDERS: [&str; 3] = ["Downloads", "Desktop", "Inbox"];

/// How to read the paths given to ingest.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IntakeOptions {
    /// Each entry inside a folder argument is its own item, rather than the folder being one.
    pub each: bool,
    /// A folder argument is one item even when its name suggests a pile, such as Downloads.
    pub whole: bool,
}

/// Why ingest couldn't go ahead.
#[derive(Debug)]
pub enum IngestError {
    /// The path doesn't exist.
    Missing(PathBuf),
    /// The path is already inside the archive.
    InsideArchive(PathBuf),
    /// The folder's name suggests a pile of separate items (such as Downloads), so the owner
    /// must say whether it's one item ([`IntakeOptions::whole`]) or many
    /// ([`IntakeOptions::each`]).
    NeedsChoice(PathBuf),
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// Reading the archive failed.
    Archive(ArchiveError),
    /// Building a folder's manifest failed.
    Manifest(ManifestError),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(p) => write!(f, "{} doesn't exist", p.display()),
            Self::InsideArchive(p) => write!(f, "{} is already in the archive", p.display()),
            Self::NeedsChoice(p) => write!(
                f,
                "{} usually holds separate things: use --each to ingest each entry, or --whole to ingest it as one folder",
                p.display()
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Archive(e) => e.fmt(f),
            Self::Manifest(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for IngestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Archive(e) => Some(e),
            Self::Manifest(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ArchiveError> for IngestError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

impl From<ManifestError> for IngestError {
    fn from(e: ManifestError) -> Self {
        Self::Manifest(e)
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> IngestError + '_ {
    move |source| IngestError::Io {
        path: path.to_owned(),
        source,
    }
}

/// A path intake left out, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The path.
    pub path: PathBuf,
    /// Why it was left out.
    pub reason: String,
}

/// The paths that become items: each argument, or with [`IntakeOptions::each`] each entry
/// inside a folder argument. Hidden entries, links and ignored files (§2.8) inside a folder
/// are skipped with a reason; arguments that are links are skipped too, since the item is
/// what they point to.
///
/// # Errors
///
/// [`IngestError::Missing`], [`IngestError::InsideArchive`], [`IngestError::NeedsChoice`], or
/// an error reading a folder.
pub fn intake(
    archive: &Archive,
    paths: &[PathBuf],
    options: IntakeOptions,
) -> Result<(Vec<PathBuf>, Vec<Skipped>), IngestError> {
    let root = std::fs::canonicalize(archive.root()).map_err(io(archive.root()))?;
    let mut items: Vec<PathBuf> = Vec::new();
    let mut skipped = Vec::new();
    for given in paths {
        let metadata =
            std::fs::symlink_metadata(given).map_err(|_| IngestError::Missing(given.clone()))?;
        if metadata.file_type().is_symlink() {
            skipped.push(Skipped {
                path: given.clone(),
                reason: "a link; ingest what it points to instead".into(),
            });
            continue;
        }
        let path = std::fs::canonicalize(given).map_err(io(given))?;
        if path.starts_with(&root) {
            return Err(IngestError::InsideArchive(given.clone()));
        }
        if !metadata.is_dir() || !options.each {
            let is_pile = metadata.is_dir()
                && path.file_name().is_some_and(|name| {
                    PILE_FOLDERS
                        .iter()
                        .any(|pile| name.to_string_lossy().eq_ignore_ascii_case(pile))
                });
            if is_pile && !options.whole {
                return Err(IngestError::NeedsChoice(given.clone()));
            }
            items.push(path);
            continue;
        }
        let mut entries: Vec<PathBuf> = std::fs::read_dir(&path)
            .map_err(io(&path))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        entries.sort();
        for entry in entries {
            let name = entry
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let reason = if is_ignored(&name) {
                Some("ignored (§2.8)")
            } else if name.starts_with('.') {
                Some("hidden")
            } else if std::fs::symlink_metadata(&entry).is_ok_and(|m| m.file_type().is_symlink()) {
                Some("a link")
            } else {
                None
            };
            match reason {
                Some(reason) => skipped.push(Skipped {
                    path: entry,
                    reason: reason.into(),
                }),
                None => items.push(entry),
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    items.retain(|p| seen.insert(p.clone()));
    Ok((items, skipped))
}

/// An item ingest has examined: what it is and its fingerprint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Where it is (absolute).
    pub path: PathBuf,
    /// Its kind: file, open folder or sealed bundle.
    pub kind: Kind,
    /// How a bundle is sealed.
    pub sealed: Option<Sealing>,
    /// The project markers that made it a bundle.
    pub markers: Vec<String>,
    /// A file's checksum, or a folder's manifest content checksum (§7.7).
    pub checksum: Checksum,
    /// The size in bytes, or a folder's total, leaving out regenerable content.
    pub size: u64,
    /// An archived item with the same fingerprint.
    pub duplicate_of: Option<Duplicate>,
    /// An earlier candidate in the same batch with the same fingerprint, by position.
    pub same_as: Option<usize>,
}

/// The archived item a candidate duplicates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    /// The item's identifier.
    pub id: Id,
    /// Its card's path in the archive.
    pub card: String,
}

/// Finds what `path` is and fingerprints it.
///
/// # Errors
///
/// An error reading the item or building a folder's manifest.
pub fn examine(archive: &Archive, path: &Path) -> Result<Candidate, IngestError> {
    let detected = archive.detect(path)?;
    let (checksum, size) = if detected.kind == Kind::File {
        let checksum = Checksum::of_file(path).map_err(io(path))?;
        let size = std::fs::metadata(path).map_err(io(path))?.len();
        (checksum, size)
    } else {
        let rules = archive.rules().map_err(|e| IngestError::Io {
            path: archive.root().join("_System/rules"),
            source: std::io::Error::other(e.to_string()),
        })?;
        let manifest = Manifest::build(path, Id::new_v7(), &rules, None)?;
        (manifest.checksum(), manifest.total_size())
    };
    Ok(Candidate {
        path: path.to_owned(),
        kind: detected.kind,
        sealed: detected.sealed,
        markers: detected.markers,
        checksum,
        size,
        duplicate_of: None,
        same_as: None,
    })
}

/// The archive's items by fingerprint: files by checksum, folders and bundles by manifest
/// content checksum.
#[derive(Debug, Clone, Default)]
pub struct ChecksumIndex {
    items: HashMap<Checksum, Duplicate>,
}

impl ChecksumIndex {
    /// Reads every card in `archive`. Cards that don't parse are left out, as they are from
    /// [`Archive::items`].
    ///
    /// # Errors
    ///
    /// An error listing the archive or reading a card.
    pub fn build(archive: &Archive) -> Result<Self, IngestError> {
        let mut items = HashMap::new();
        for item in archive.items()? {
            let path = archive.root().join(&item.card);
            let text = std::fs::read_to_string(&path).map_err(io(&path))?;
            let Ok(file) = CardFile::parse(&text) else {
                continue;
            };
            let card = file.card();
            let fingerprint = card
                .checksum
                .clone()
                .or_else(|| card.manifest.as_ref().map(|m| m.checksum.clone()));
            if let Some(fingerprint) = fingerprint {
                items.entry(fingerprint).or_insert(Duplicate {
                    id: item.id,
                    card: item.card.clone(),
                });
            }
        }
        Ok(Self { items })
    }

    /// The archived item with this fingerprint, if any.
    #[must_use]
    pub fn get(&self, checksum: &Checksum) -> Option<&Duplicate> {
        self.items.get(checksum)
    }

    /// How many items are indexed.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether no items are indexed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Marks candidates that duplicate an archived item, or an earlier candidate in the batch.
pub fn mark_duplicates(index: &ChecksumIndex, candidates: &mut [Candidate]) {
    let mut first: HashMap<Checksum, usize> = HashMap::new();
    for (i, candidate) in candidates.iter_mut().enumerate() {
        candidate.duplicate_of = index.get(&candidate.checksum).cloned();
        match first.get(&candidate.checksum) {
            Some(&earlier) => candidate.same_as = Some(earlier),
            None => {
                first.insert(candidate.checksum.clone(), i);
            }
        }
    }
}

/// Stages 1 to 4 for a batch: intake, kind, fingerprint and duplicates.
///
/// # Errors
///
/// Any [`IngestError`] from those stages.
pub fn plan(
    archive: &Archive,
    paths: &[PathBuf],
    options: IntakeOptions,
) -> Result<(Vec<Candidate>, Vec<Skipped>), IngestError> {
    let (items, skipped) = intake(archive, paths, options)?;
    let mut candidates = items
        .iter()
        .map(|path| examine(archive, path))
        .collect::<Result<Vec<_>, _>>()?;
    let index = ChecksumIndex::build(archive)?;
    mark_duplicates(&index, &mut candidates);
    Ok((candidates, skipped))
}

/// An ingest batch: where its proposals go and who is making them.
#[derive(Debug, Clone, Copy)]
pub struct Batch<'a> {
    /// The archive.
    pub archive: &'a Archive,
    /// Its collections.
    pub collections: &'a [crate::archive::Collection],
    /// The batch's identifier.
    pub id: Id,
    /// The device drafting.
    pub device: Id,
}

impl Batch<'_> {
    /// The sensitivity an item gets at `route`'s destination before any rule or model looks
    /// at it: the collection's `sensitivity_default`, else the archive's, else `personal`.
    #[must_use]
    pub fn default_sensitivity(&self, route: &route::Route) -> crate::card::Sensitivity {
        self.collections
            .iter()
            .find(|c| Some(c.doc.id) == route.collection)
            .and_then(|c| c.doc.extra.get("sensitivity_default"))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .or(self.archive.doc().sensitivity_default)
            .unwrap_or(crate::card::Sensitivity::Personal)
    }

    /// The review entry proposing to file `candidate` as `draft`, where `route` says (stage
    /// 11). Tags are narrowed to those that apply at the destination, the model's new tag (if
    /// any) is proposed, and the entry needs the owner when the card couldn't be settled or
    /// repeats a restricted value `sensitivity` found.
    /// `None` when the route has no collection, as in an archive without Holding.
    #[must_use]
    pub fn propose(
        &self,
        candidate: &Candidate,
        draft: &card::Draft,
        route: &route::Route,
        sensitivity: &sensitivity::Assessment,
        earlier_attempts: &[crate::review::Attempt],
    ) -> Option<ReviewEntry> {
        let (archive, collections, batch, device) =
            (self.archive, self.collections, self.id, self.device);

        let collection = route.collection?;
        let mut problems: Vec<Problem> = draft
            .problems
            .iter()
            .map(|message| Problem {
                stage: "card".into(),
                message: message.clone(),
            })
            .collect();
        let tags = route::tags_for(archive, collections, &route.path, &draft.tags);
        let dropped: Vec<&str> = draft
            .tags
            .iter()
            .filter(|t| !tags.contains(t))
            .map(|t| t.0.as_str())
            .collect();
        if !dropped.is_empty() {
            problems.push(Problem {
                stage: "route".into(),
                message: format!(
                    "tags {} don't apply in {}, so they were left off",
                    dropped.join(", "),
                    route.path
                ),
            });
        }
        let repeated = sensitivity.matched.iter().any(|value| {
            draft.title.contains(value.as_str())
                || draft
                    .summary
                    .as_deref()
                    .is_some_and(|s| s.contains(value.as_str()))
        });
        if repeated {
            problems.push(Problem {
                stage: "sensitivity".into(),
                message:
                    "the card repeats an account or identity number from the item; edit it out"
                        .into(),
            });
        }
        let mut attempts = earlier_attempts.to_vec();
        attempts.extend(sensitivity.attempts.iter().cloned());
        attempts.extend(draft.attempts.iter().cloned());
        attempts.extend(route.attempts.iter().cloned());
        let state = if draft.problems.is_empty() && !repeated {
            State::Proposed
        } else {
            State::NeedsOwner
        };
        Some(ReviewEntry {
            carrel: ReviewMarker::Review,
            format: crate::FORMAT_VERSION,
            id: Id::new_v7(),
            batch,
            created: crate::card::DateOrTime(
                jiff::Timestamp::now()
                    .to_zoned(jiff::tz::TimeZone::system())
                    .strftime("%Y-%m-%dT%H:%M:%S%:z")
                    .to_string(),
            ),
            state,
            source: Source {
                path: candidate.path.clone(),
                device,
                checksum: candidate.checksum.clone(),
                size: candidate.size,
                extra: crate::card::Extra::new(),
            },
            kind: candidate.kind,
            sealed: candidate.sealed,
            duplicate_of: candidate.duplicate_of.as_ref().map(|d| d.id),
            card: CardDraft {
                title: draft.title.clone(),
                summary: draft.summary.clone(),
                tags: tags.clone(),
                dates: draft.created.clone().map(|created| crate::card::Dates {
                    created: Some(created),
                    ..crate::card::Dates::default()
                }),
                sensitivity: sensitivity.level,
                generated: draft.generated.clone(),
                extra: crate::card::Extra::new(),
            },
            proposed_tags: draft.new_tag.clone().into_iter().collect(),
            destination: Destination {
                collection,
                path: route.path.clone(),
                confidence: route.confidence,
                reasons: route.reasons.clone(),
                rule: route.rule.clone(),
                extra: crate::card::Extra::new(),
            },
            problems,
            attempts,
            sensitivity_reasons: sensitivity.reasons.clone(),
            extra: crate::card::Extra::new(),
        })
    }
}
