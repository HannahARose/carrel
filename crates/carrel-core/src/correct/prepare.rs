//! Preparing a typed correction (`docs/design/corrections.md`): the owner's words worked out
//! into field changes on a proposal or a filed item, held until the owner says yes, then
//! saved. Every interface shows the change in its own way, from [`Prepared`]'s fields before
//! and after, and asks in its own way.

use std::path::{Path, PathBuf};

use crate::actions::{self, ActionError};
use crate::archive::{Archive, ArchiveError, Collection, ItemRef};
use crate::card::{Checksum, DateOrTime, Kind, Sensitivity};
use crate::card_file::{CardFile, CardFileError};
use crate::coordinator::{
    Coordinator as _, CoordinatorError, DEFAULT_LEASE, DiskLock, Holder, WriterLease,
};
use crate::correct::{Correctable as _, CorrectionError, Fact, Fields, Subject, Worked};
use crate::history::{Change, History, HistoryError, Operation};
use crate::ingest::Candidate;
use crate::ingest::card::Vocabulary;
use crate::providers::Registry;
use crate::review::{ReviewEntry, ReviewError, ReviewQueue};

/// A correction worked out, waiting for the owner's yes. Nothing is saved until [`save`].
#[derive(Debug)]
pub struct Prepared {
    /// The card's title, for a heading.
    pub title: String,
    /// The card's fields before.
    pub before: Fields,
    /// Its fields after.
    pub after: Fields,
    /// The collection it moves from and to, when it moves.
    pub moved: Option<(String, String)>,
    /// What the words were worked out into, and by which model.
    pub worked: Worked,
    /// The owner's words.
    pub words: String,
    /// The fields changed, by name.
    pub changed: Vec<&'static str>,
    /// Whether a lower sensitivity was kept at sensitive, because a rule finds a restricted
    /// value in the item.
    pub kept_sensitive: bool,
    /// Tags left off because they don't apply in the collection it moves to.
    pub dropped: Vec<String>,
    /// Who said it, as the change's summary names them: the owner, unless
    /// [`Prepared::said_by`] says otherwise.
    by: String,
    pending: Pending,
}

/// What a prepared correction saves.
#[derive(Debug)]
enum Pending {
    /// A proposal, corrected.
    Proposal(Box<ReviewEntry>),
    /// A filed item's card, corrected, refiled when it moves.
    Filed {
        item: ItemRef,
        card_path: PathBuf,
        file: Box<CardFile>,
    },
}

impl Prepared {
    /// Whether it changes nothing anyone would see: no field the owner can read, and no move.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let changes = &self.worked.changes;
        changes.title.is_none()
            && changes.summary.is_none()
            && changes.created.is_none()
            && changes.tags.is_none()
            && self.before.sensitivity == self.after.sensitivity
            && self.moved.is_none()
    }

    /// A lasting fact the owner stated, to offer for the glossary once it's saved.
    #[must_use]
    pub fn fact(&self) -> Option<&Fact> {
        self.worked.changes.fact.as_ref()
    }

    /// The collection it moves to, when it moves.
    #[must_use]
    pub fn to(&self) -> Option<&str> {
        self.moved.as_ref().map(|(_, to)| to.as_str())
    }

    /// Says the words were someone else's than the owner's, such as `Librarian`, as the
    /// change's summary names them.
    #[must_use]
    pub fn said_by(mut self, by: &str) -> Self {
        by.clone_into(&mut self.by);
        self
    }

    /// The corrected proposal, when it's a proposal that's corrected.
    #[must_use]
    pub fn proposal(&self) -> Option<&ReviewEntry> {
        match &self.pending {
            Pending::Proposal(entry) => Some(entry),
            Pending::Filed { .. } => None,
        }
    }
}

/// What saving a correction did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// The card's title.
    pub title: String,
    /// Whether it was a filed item, so the change can be undone from the history.
    pub filed: bool,
    /// The history commit, for a filed item.
    pub commit: Option<String>,
    /// The card's path after, for a filed item.
    pub card: Option<String>,
}

/// Why a correction couldn't be prepared or saved.
#[derive(Debug)]
pub enum PrepareError {
    /// There's no proposal with this identifier.
    NoProposal(String),
    /// The words couldn't be worked out.
    Correction(CorrectionError),
    /// The proposal couldn't be read or corrected.
    Action(ActionError),
    /// The proposal couldn't be saved.
    Queue(ReviewError),
    /// The archive couldn't be read.
    Archive(ArchiveError),
    /// The card couldn't be read or written.
    Io(std::io::Error),
    /// The card couldn't be parsed or written out.
    Card(CardFileError),
    /// The writer lease couldn't be taken or released.
    Lease(CoordinatorError),
    /// The item couldn't be refiled.
    Refile(String),
    /// The history couldn't be written.
    History(HistoryError),
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoProposal(id) => write!(f, "no proposal {id}"),
            Self::Correction(e) => write!(f, "{e}"),
            Self::Action(e) => write!(f, "{e}"),
            Self::Queue(e) => write!(f, "{e}"),
            Self::Archive(e) => write!(f, "{e}"),
            Self::Io(e) => write!(f, "{e}"),
            Self::Card(e) => write!(f, "{e}"),
            Self::Lease(e) => write!(f, "{e}"),
            Self::Refile(message) => f.write_str(message),
            Self::History(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for PrepareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::NoProposal(_) | Self::Refile(_) => None,
            Self::Correction(e) => Some(e),
            Self::Action(e) => Some(e),
            Self::Queue(e) => Some(e),
            Self::Archive(e) => Some(e),
            Self::Io(e) => Some(e),
            Self::Card(e) => Some(e),
            Self::Lease(e) => Some(e),
            Self::History(e) => Some(e),
        }
    }
}

macro_rules! from {
    ($($error:ty => $variant:ident),* $(,)?) => {
        $(impl From<$error> for PrepareError {
            fn from(error: $error) -> Self {
                Self::$variant(error)
            }
        })*
    };
}

from!(
    CorrectionError => Correction,
    ActionError => Action,
    ReviewError => Queue,
    ArchiveError => Archive,
    std::io::Error => Io,
    CardFileError => Card,
    CoordinatorError => Lease,
    HistoryError => History,
);

/// The context a correction is worked out in: the archive's collections and tags.
struct Context {
    collections: Vec<Collection>,
    described: Vec<(String, String)>,
    vocabulary: Vocabulary,
}

impl Context {
    fn of(archive: &Archive) -> Result<Self, ArchiveError> {
        let collections = archive.collections()?;
        Ok(Self {
            vocabulary: Vocabulary::of(archive, &collections),
            described: described(&collections),
            collections,
        })
    }
}

/// The collections as the model sees them, by path with what each is for.
fn described(collections: &[Collection]) -> Vec<(String, String)> {
    collections
        .iter()
        .map(|c| {
            let purpose = c
                .doc
                .description
                .clone()
                .unwrap_or_else(|| c.doc.title.clone());
            (c.path.clone(), purpose)
        })
        .collect()
}

/// Works out what `words` change in the proposal `id`, with `registry`'s text models, without
/// saving anything.
///
/// # Errors
///
/// [`PrepareError`] if the proposal isn't found, or the correction can't be worked out.
pub fn prepare_review(
    archive: &Archive,
    registry: &Registry,
    id: &str,
    words: &str,
) -> Result<Prepared, PrepareError> {
    let entry = actions::proposals(archive, &[id.to_owned()])?
        .pop()
        .ok_or_else(|| PrepareError::NoProposal(id.to_owned()))?;
    let context = Context::of(archive)?;
    let before = entry.card.fields();
    let earlier: Vec<String> = crate::correct::notes(&before)
        .into_iter()
        .map(|n| n.said)
        .collect();
    let text = item_text(archive, registry, &entry.source.path, entry.kind);
    let subject = Subject {
        card: &before,
        collection: &entry.destination.path,
        text: &text,
        collections: &context.described,
        vocabulary: &context.vocabulary,
    };
    let worked = crate::correct::work_out(registry, &subject, words, &earlier)?;
    let corrected = actions::correct_proposal(archive, registry, &entry, &worked, words)?;
    let after = corrected.entry.card.fields();
    Ok(Prepared {
        title: entry.card.title.clone(),
        before,
        after,
        moved: corrected.to.map(|to| (entry.destination.path.clone(), to)),
        worked,
        words: words.to_owned(),
        changed: corrected.changed,
        kept_sensitive: corrected.kept_sensitive,
        dropped: corrected.dropped,
        by: OWNER.into(),
        pending: Pending::Proposal(Box::new(corrected.entry)),
    })
}

/// Works out what `words` change in the filed item `item`, with `registry`'s text models,
/// without saving anything.
///
/// # Errors
///
/// [`PrepareError`] if its card can't be read, or the correction can't be worked out.
pub fn prepare_filed(
    archive: &Archive,
    registry: &Registry,
    item: ItemRef,
    words: &str,
) -> Result<Prepared, PrepareError> {
    filed(archive, registry, item, words, |subject, earlier| {
        crate::correct::work_out(registry, subject, words, earlier)
    })
}

/// Prepares `lines` of `field: value`, worked out by `model` rather than said by the owner,
/// such as the librarian's, for the filed item `item`, without saving anything. Every field
/// stays model-written, and it's checked, refiled and saved exactly as a correction the owner
/// typed.
///
/// # Errors
///
/// [`PrepareError`] if its card can't be read, or a line isn't `field: value` with a value
/// the field takes.
pub fn prepare_filed_lines(
    archive: &Archive,
    registry: &Registry,
    item: ItemRef,
    lines: &str,
    model: &str,
) -> Result<Prepared, PrepareError> {
    filed(archive, registry, item, lines, |subject, _| {
        let changes = crate::correct::parse_lines(lines, subject)?.ok_or_else(|| {
            CorrectionError::BadLine(format!("“{lines}” isn't a `field: value` line"))
        })?;
        Ok(Worked {
            changes: changes.inferred(),
            model: Some(model.to_owned()),
            attempts: Vec::new(),
        })
    })
}

/// Prepares a correction to the filed item `item`, with its changes from `work`, given the
/// card as the model sees it and the owner's earlier corrections.
fn filed(
    archive: &Archive,
    registry: &Registry,
    item: ItemRef,
    words: &str,
    work: impl FnOnce(&Subject<'_>, &[String]) -> Result<Worked, CorrectionError>,
) -> Result<Prepared, PrepareError> {
    let card_path = archive.root().join(&item.card);
    let mut file = CardFile::parse(&std::fs::read_to_string(&card_path)?)?;
    let context = Context::of(archive)?;
    let before = file.card().fields();
    let earlier: Vec<String> = crate::correct::notes(&before)
        .into_iter()
        .map(|n| n.said)
        .collect();
    let here = item.card.rsplit_once('/').map_or("", |(f, _)| f).to_owned();
    let contents = item.item.as_ref().map(|p| archive.root().join(p));
    let text = contents
        .as_deref()
        .map(|p| item_text(archive, registry, p, item.kind))
        .unwrap_or_default();
    let subject = Subject {
        card: &before,
        collection: &here,
        text: &text,
        collections: &context.described,
        vocabulary: &context.vocabulary,
    };
    let worked = work(&subject, &earlier)?;
    let (after, changed, kept_sensitive) =
        corrected(&before, &worked, words, contents.as_deref(), &text);
    let to = target_of(
        archive,
        registry,
        &context.collections,
        &item,
        contents.as_deref(),
        &after,
        &worked,
    )
    .filter(|t| *t != here);
    file.card_mut().set_fields(after.clone());
    Ok(Prepared {
        title: before.title.clone(),
        before,
        after,
        moved: to.map(|to| (here, to)),
        worked,
        words: words.to_owned(),
        changed,
        kept_sensitive,
        dropped: Vec::new(),
        by: OWNER.into(),
        pending: Pending::Filed {
            item,
            card_path,
            file: Box::new(file),
        },
    })
}

/// Who a correction's words are from, unless [`Prepared::said_by`] says otherwise.
const OWNER: &str = "Owner";

/// Saves a prepared correction: a proposal back to the queue, or a filed item's card as one
/// change, refiled when its collection changed, made as the writer `holder` gives. `holder` is
/// only asked for a filed item: a proposal is saved without the writer lease.
///
/// # Errors
///
/// [`PrepareError`] if it can't be written or committed, or `holder`'s error.
pub fn save<E: From<PrepareError>>(
    archive: &Archive,
    prepared: Prepared,
    holder: impl FnOnce() -> Result<Holder, E>,
) -> Result<Saved, E> {
    match prepared.pending {
        Pending::Proposal(entry) => {
            ReviewQueue::of(archive)
                .save(&entry)
                .map_err(PrepareError::from)?;
            Ok(Saved {
                title: entry.card.title.clone(),
                filed: false,
                commit: None,
                card: None,
            })
        }
        Pending::Filed { .. } => {
            let lease = DiskLock::for_archive(archive)
                .acquire(holder()?, "correct", DEFAULT_LEASE)
                .map_err(PrepareError::from)?;
            let saved = save_holding(archive, prepared, &lease);
            lease.release().map_err(PrepareError::from)?;
            Ok(saved?)
        }
    }
}

/// Saves a prepared correction as [`save`] does, with the writer `lease` the caller already
/// holds, so the change is marked as the lease marks it ([`WriterLease::act_as`]).
///
/// # Errors
///
/// [`PrepareError`] if it can't be written or committed.
pub fn save_holding(
    archive: &Archive,
    prepared: Prepared,
    lease: &WriterLease,
) -> Result<Saved, PrepareError> {
    let to = prepared.to().map(str::to_owned);
    match prepared.pending {
        Pending::Proposal(entry) => {
            ReviewQueue::of(archive).save(&entry)?;
            Ok(Saved {
                title: entry.card.title.clone(),
                filed: false,
                commit: None,
                card: None,
            })
        }
        Pending::Filed {
            item,
            card_path,
            file,
        } => {
            let title = file.card().title.clone();
            let (card, commit) = save_filed(
                archive,
                Filing {
                    item: &item,
                    card_path: &card_path,
                    target: to.as_deref(),
                    words: &prepared.words,
                    changed: &prepared.changed,
                    by: &prepared.by,
                },
                *file,
                lease,
            )?;
            Ok(Saved {
                title,
                filed: true,
                commit,
                card: Some(card),
            })
        }
    }
}

/// The item's text, as drafting read it, for a model's context; empty when it can't be read.
#[must_use]
pub fn item_text(archive: &Archive, registry: &Registry, path: &Path, kind: Kind) -> String {
    let Ok(rules) = archive.rules() else {
        return String::new();
    };
    crate::ingest::text::read(registry, &rules, &candidate(path.to_owned(), kind)).text
}

/// A candidate for the item at `path`, enough to read or route it again.
fn candidate(path: PathBuf, kind: Kind) -> Candidate {
    Candidate {
        path,
        kind,
        sealed: None,
        markers: Vec::new(),
        checksum: Checksum(String::new()),
        size: 0,
        duplicate_of: None,
        likely: Vec::new(),
        same_as: None,
        inside: None,
    }
}

/// `before` with the correction applied, the fields changed, and whether a lower sensitivity
/// was kept at sensitive because a rule finds a restricted value in the item at `contents`,
/// read as `text`.
fn corrected(
    before: &Fields,
    worked: &Worked,
    words: &str,
    contents: Option<&Path>,
    text: &str,
) -> (Fields, Vec<&'static str>, bool) {
    use crate::ingest::sensitivity::{floor, restricted_by_rule};

    let mut after = before.clone();
    let at = DateOrTime(
        jiff::Zoned::now()
            .strftime("%Y-%m-%dT%H:%M:%S%:z")
            .to_string(),
    );
    let changed = crate::correct::apply(&mut after, worked, words, &at);
    let name = contents
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let kept = after.sensitivity < Sensitivity::Sensitive
        && floor(&name, text, Sensitivity::Public)
            .reasons
            .iter()
            .any(|r| restricted_by_rule(r));
    if kept {
        after.sensitivity = Sensitivity::Sensitive;
    }
    (after, changed, kept)
}

/// A filed item's correction, as [`save_filed`] writes it.
#[derive(Clone, Copy)]
struct Filing<'a> {
    item: &'a ItemRef,
    card_path: &'a Path,
    /// The collection to refile it into, if it moves.
    target: Option<&'a str>,
    words: &'a str,
    changed: &'a [&'static str],
    /// Who said the words.
    by: &'a str,
}

/// Writes a filed item's corrected card, refiling it when it moves, as one change described
/// with the owner's words. Returns the card's path and the commit.
fn save_filed(
    archive: &Archive,
    filing: Filing<'_>,
    file: CardFile,
    lease: &WriterLease,
) -> Result<(String, Option<String>), PrepareError> {
    let mut said: String = filing
        .words
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if said.chars().count() > 200 {
        said = said.chars().take(200).collect::<String>() + "…";
    }
    let summary = format!(
        "Correct “{}”: {}. {}: {said}",
        file.card().title,
        filing.changed.join(", "),
        filing.by
    );
    if let Some(to) = filing.target {
        let moved = crate::rename::refile(archive, filing.item, to, file, &summary, lease)
            .map_err(|e| PrepareError::Refile(e.to_string()))?;
        return Ok((moved.card, moved.commit));
    }
    let temp = filing.card_path.with_extension("md.tmp-correct");
    std::fs::write(&temp, file.to_text()?)?;
    std::fs::rename(&temp, filing.card_path)?;
    let mut change = Change::new(
        Operation::Edit,
        summary,
        lease.info().person,
        lease.info().device,
    );
    change.items = vec![filing.item.id];
    Ok((
        filing.item.card.clone(),
        History::open(archive)?.commit(archive, &change, lease)?,
    ))
}

/// The collection a correction sends a filed item to: one the owner named, or, when what it is
/// changed, wherever it's routed now.
fn target_of(
    archive: &Archive,
    registry: &Registry,
    collections: &[Collection],
    item: &ItemRef,
    contents: Option<&Path>,
    fields: &Fields,
    worked: &Worked,
) -> Option<String> {
    if let Some(named) = &worked.changes.collection {
        return Some(named.value.clone());
    }
    worked
        .changes
        .reroute
        .then(|| reroute(archive, registry, collections, item, contents, fields))
        .flatten()
}

/// Where a corrected item belongs now, routed again from its corrected card.
fn reroute(
    archive: &Archive,
    registry: &Registry,
    collections: &[Collection],
    item: &ItemRef,
    contents: Option<&Path>,
    fields: &Fields,
) -> Option<String> {
    use crate::ingest::route::{Router, RoutingRules};
    let rules = RoutingRules::of(archive, collections);
    let router = Router::new(archive, collections, &rules);
    let path = contents.map_or_else(|| archive.root().join(&item.card), Path::to_owned);
    let candidate = candidate(path.clone(), item.kind);
    let draft = crate::ingest::card::Draft {
        title: fields.title.clone(),
        summary: fields.summary.clone(),
        tags: fields.tags.clone(),
        new_tag: None,
        created: fields.dates.as_ref().and_then(|d| d.created.clone()),
        generated: None,
        attempts: Vec::new(),
        problems: Vec::new(),
    };
    let source = path.parent().unwrap_or(&path).to_owned();
    let route = router.route(registry, &candidate, &source, &draft);
    // Only somewhere it fits: an unsure answer, or Holding, leaves it where it is.
    (route.collection.is_some()
        && route.confidence != crate::review::Confidence::Low
        && route.path != "Holding")
        .then_some(route.path)
}
