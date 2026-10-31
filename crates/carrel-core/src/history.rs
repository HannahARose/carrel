//! The archive's metadata history (FORMAT.md §8): a Git repository in `_System/history.git`
//! whose work tree is the archive root.
//!
//! History tracks metadata only (cards, design documents, changelogs, the Journal, `Active/`
//! and `_System/` data), never item contents, which the backup system versions. Carrel stages
//! exactly the metadata files it computes from the archive, rather than relying on ignore
//! patterns, so uncataloged files and item contents never end up in history.
//!
//! Every change is one commit with `Carrel-*` trailers. Undoing a change makes a new commit
//! that reverses it, moves any items it moved back to where they were, and records which
//! commit it reverted. History is never rewritten.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use git2::{Oid, Repository, RepositoryInitOptions, Signature};
use serde::{Deserialize, Serialize};

use crate::archive::{Archive, ArchiveError};
use crate::card::Id;
use crate::coordinator::{CoordinatorError, WriterLease};

/// Where the history repository lives, relative to the archive root.
pub const HISTORY_DIR: &str = "_System/history.git";

/// Folders under `_System/` that history never tracks (FORMAT.md §8.1).
const UNTRACKED_SYSTEM: [&str; 4] = ["history.git", "manifests", "index", "cache.nosync"];

/// The kind of change a commit records (FORMAT.md §8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    /// The archive was created.
    Create,
    /// Items were ingested.
    Ingest,
    /// Proposals or entries were approved.
    Approve,
    /// Cards or design documents were edited.
    Edit,
    /// Items were moved.
    Move,
    /// An item was checked out.
    Checkout,
    /// An item was returned.
    Return,
    /// A work session was recorded.
    Session,
    /// A section was restructured.
    Restructure,
    /// An item was deaccessioned.
    Deaccession,
    /// An item was purged.
    Purge,
    /// A change made outside Carrel was recorded.
    OutsideEdit,
    /// The archive format was migrated.
    Migrate,
    /// Inbox entries from other devices were applied.
    Inbox,
}

impl Operation {
    /// Every operation, in FORMAT.md's order.
    pub const ALL: [Self; 14] = [
        Self::Create,
        Self::Ingest,
        Self::Approve,
        Self::Edit,
        Self::Move,
        Self::Checkout,
        Self::Return,
        Self::Session,
        Self::Restructure,
        Self::Deaccession,
        Self::Purge,
        Self::OutsideEdit,
        Self::Migrate,
        Self::Inbox,
    ];

    /// The name used in the `Carrel-Operation` trailer.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Ingest => "ingest",
            Self::Approve => "approve",
            Self::Edit => "edit",
            Self::Move => "move",
            Self::Checkout => "checkout",
            Self::Return => "return",
            Self::Session => "session",
            Self::Restructure => "restructure",
            Self::Deaccession => "deaccession",
            Self::Purge => "purge",
            Self::OutsideEdit => "outside-edit",
            Self::Migrate => "migrate",
            Self::Inbox => "inbox",
        }
    }

    /// The operation named `name`, if there is one.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|op| op.as_str() == name)
    }
}

/// An item moved by a change: recorded so undo can move it back (FORMAT.md §8.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    /// The item moved.
    pub item: Id,
    /// Where it was, relative to the archive root.
    pub from: String,
    /// Where it went, relative to the archive root.
    pub to: String,
}

/// A change to record as one commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// What kind of change this is.
    pub operation: Operation,
    /// A one-line summary.
    pub summary: String,
    /// Optional longer explanation.
    pub body: Option<String>,
    /// The person who made the change.
    pub person: Id,
    /// The device that made it.
    pub device: Id,
    /// The backup snapshot taken with the change, such as `kopia:k3f9a1c0`.
    pub snapshot: Option<String>,
    /// The items the change concerns.
    pub items: Vec<Id>,
    /// Items moved by the change, in the order they were moved.
    pub moves: Vec<Move>,
}

impl Change {
    /// A change with only the required parts.
    #[must_use]
    pub fn new(operation: Operation, summary: impl Into<String>, person: Id, device: Id) -> Self {
        Self {
            operation,
            summary: summary.into(),
            body: None,
            person,
            device,
            snapshot: None,
            items: Vec::new(),
            moves: Vec::new(),
        }
    }

    fn message(&self, reverts: Option<Oid>) -> String {
        let mut msg = self
            .summary
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        if let Some(body) = &self.body {
            msg.push_str("\n\n");
            msg.push_str(body.trim());
        }
        msg.push_str("\n\n");
        let _ = writeln!(msg, "Carrel-Operation: {}", self.operation.as_str());
        let _ = writeln!(msg, "Carrel-Person: {}", self.person);
        let _ = writeln!(msg, "Carrel-Device: {}", self.device);
        if let Some(snapshot) = &self.snapshot {
            let _ = writeln!(msg, "Carrel-Snapshot: {}", snapshot.trim());
        }
        for item in &self.items {
            let _ = writeln!(msg, "Carrel-Items: {item}");
        }
        for m in &self.moves {
            let json = serde_json::to_string(m).expect("moves always serialize");
            let _ = writeln!(msg, "Carrel-Move: {json}");
        }
        if let Some(oid) = reverts {
            let _ = writeln!(msg, "Carrel-Reverts: {oid}");
        }
        msg
    }
}

/// One commit in the history, as read back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The commit's identifier (hex).
    pub id: String,
    /// The summary line.
    pub summary: String,
    /// The operation, if the trailer is present and known.
    pub operation: Option<Operation>,
    /// The person, if recorded.
    pub person: Option<Id>,
    /// The device, if recorded.
    pub device: Option<Id>,
    /// The backup snapshot, if recorded.
    pub snapshot: Option<String>,
    /// The items concerned.
    pub items: Vec<Id>,
    /// Items moved.
    pub moves: Vec<Move>,
    /// The commit this one reverted, if it is an undo.
    pub reverts: Option<String>,
    /// When the commit was made, as seconds since the Unix epoch.
    pub time: i64,
}

/// Why a history operation failed.
#[derive(Debug)]
pub enum HistoryError {
    /// Git reported an error.
    Git(git2::Error),
    /// Listing the archive's metadata failed.
    Archive(ArchiveError),
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The archive has no history repository yet.
    NotInitialized,
    /// The archive already has a history repository.
    AlreadyInitialized,
    /// The commit to undo doesn't exist.
    UnknownCommit(String),
    /// Undoing would conflict with later changes to the same lines; nothing was changed.
    UndoConflict(Vec<String>),
    /// An item can't be moved back because its old location is taken or it is missing.
    UndoBlocked(String),
    /// The writer lease is missing, lost or expired; only the current writer may record history.
    Writer(CoordinatorError),
}

impl std::fmt::Display for HistoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Git(e) => write!(f, "history: {e}"),
            Self::Archive(e) => write!(f, "history: {e}"),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::NotInitialized => f.write_str("the archive has no history repository yet"),
            Self::AlreadyInitialized => f.write_str("the archive already has a history repository"),
            Self::UnknownCommit(id) => write!(f, "no change {id} in the history"),
            Self::UndoConflict(paths) => {
                write!(
                    f,
                    "undoing would conflict with later changes to {}",
                    paths.join(", ")
                )
            }
            Self::UndoBlocked(why) => write!(f, "can't undo: {why}"),
            Self::Writer(e) => write!(f, "history: {e}"),
        }
    }
}

impl std::error::Error for HistoryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Git(e) => Some(e),
            Self::Archive(e) => Some(e),
            Self::Io { source, .. } => Some(source),
            Self::Writer(e) => Some(e),
            _ => None,
        }
    }
}

impl From<CoordinatorError> for HistoryError {
    fn from(e: CoordinatorError) -> Self {
        Self::Writer(e)
    }
}

impl From<git2::Error> for HistoryError {
    fn from(e: git2::Error) -> Self {
        Self::Git(e)
    }
}

impl From<ArchiveError> for HistoryError {
    fn from(e: ArchiveError) -> Self {
        Self::Archive(e)
    }
}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> HistoryError + '_ {
    move |source| HistoryError::Io {
        path: path.to_owned(),
        source,
    }
}

/// An archive's history repository.
pub struct History {
    repo: Repository,
    root: PathBuf,
}

impl std::fmt::Debug for History {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("History")
            .field("root", &self.root)
            .finish_non_exhaustive()
    }
}

impl History {
    /// Creates the history repository for `archive` and records its current metadata as the
    /// first commit, with operation `create`, made by the lease's holder.
    ///
    /// # Errors
    ///
    /// [`HistoryError::AlreadyInitialized`] if the repository exists, [`HistoryError::Writer`]
    /// if `lease` is no longer valid, or any Git, archive or file error.
    pub fn init(archive: &Archive, lease: &WriterLease) -> Result<Self, HistoryError> {
        let summary = format!("Create archive “{}”", archive.doc().name);
        Self::create(archive, lease, summary)
    }

    /// Starts history for an archive that already has content but no history repository,
    /// such as one made by hand or by another program. Records its current metadata as the
    /// first commit, with operation `create` and a summary saying history started here.
    ///
    /// # Errors
    ///
    /// As for [`History::init`].
    pub fn start(archive: &Archive, lease: &WriterLease) -> Result<Self, HistoryError> {
        let summary = format!("Start history of “{}”", archive.doc().name);
        Self::create(archive, lease, summary)
    }

    fn create(
        archive: &Archive,
        lease: &WriterLease,
        summary: String,
    ) -> Result<Self, HistoryError> {
        lease.check()?;
        let root = archive.root().to_path_buf();
        let git_dir = root.join(HISTORY_DIR);
        if git_dir.exists() {
            return Err(HistoryError::AlreadyInitialized);
        }
        let mut options = RepositoryInitOptions::new();
        options
            .no_dotgit_dir(true)
            .workdir_path(&root)
            .external_template(false)
            .initial_head("main");
        let repo = Repository::init_opts(&git_dir, &options)?;
        {
            // A relative work tree keeps the repository valid when the archive moves.
            let mut config = repo.config()?;
            config.set_str("core.worktree", "../..")?;
            config.set_bool("core.bare", false)?;
            // Paths are compared as written; the archive spec handles case rules itself.
            config.set_bool("core.precomposeunicode", true)?;
            // Metadata is stored exactly as written: LF line endings (FORMAT.md §2.1), whatever
            // the person's own Git settings say. Windows needs long paths for deep archives.
            config.set_bool("core.autocrlf", false)?;
            config.set_bool("core.longpaths", true)?;
        }
        let info_dir = git_dir.join("info");
        std::fs::create_dir_all(&info_dir).map_err(io(&info_dir))?;
        let exclude = info_dir.join("exclude");
        std::fs::write(&exclude, EXCLUDE).map_err(io(&exclude))?;

        let history = Self { repo, root };
        let change = Change::new(
            Operation::Create,
            summary,
            lease.info().person,
            lease.info().device,
        );
        history.commit(archive, &change, lease)?;
        Ok(history)
    }

    /// Opens the history repository of `archive`.
    ///
    /// # Errors
    ///
    /// [`HistoryError::NotInitialized`] if there is none, or a Git error.
    pub fn open(archive: &Archive) -> Result<Self, HistoryError> {
        let root = archive.root().to_path_buf();
        let git_dir = root.join(HISTORY_DIR);
        if !git_dir.is_dir() {
            return Err(HistoryError::NotInitialized);
        }
        let repo = Repository::open(&git_dir)?;
        repo.set_workdir(&root, false)?;
        Ok(Self { repo, root })
    }

    /// The metadata files history tracks right now, relative to the archive root, sorted.
    ///
    /// # Errors
    ///
    /// Any error listing the archive.
    pub fn tracked_paths(archive: &Archive) -> Result<Vec<String>, HistoryError> {
        let root = archive.root();
        let mut paths = Vec::new();
        for name in ["ARCHIVE.md", "README.md"] {
            if root.join(name).is_file() {
                paths.push(name.to_owned());
            }
        }
        for dir in ["Active", "Journal", "_System"] {
            collect_files(root, &root.join(dir), &mut paths)?;
        }
        for collection in archive.collections()? {
            for name in ["_collection.md", "CHANGELOG.md"] {
                let path = format!("{}/{name}", collection.path);
                if root.join(&path).is_file() {
                    paths.push(path);
                }
            }
        }
        paths.extend(archive.items()?.into_iter().map(|item| item.card));
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    /// Records the archive's current metadata as one commit describing `change`.
    /// Returns `None`, without committing, if no metadata changed. Only the holder of a valid
    /// writer lease may commit (FORMAT.md §8.4).
    ///
    /// # Errors
    ///
    /// [`HistoryError::Writer`] if `lease` is no longer valid, or any Git or archive error.
    pub fn commit(
        &self,
        archive: &Archive,
        change: &Change,
        lease: &WriterLease,
    ) -> Result<Option<String>, HistoryError> {
        lease.check()?;
        self.commit_with(archive, change, None)
    }

    fn commit_with(
        &self,
        archive: &Archive,
        change: &Change,
        reverts: Option<Oid>,
    ) -> Result<Option<String>, HistoryError> {
        let paths = Self::tracked_paths(archive)?;
        let mut index = self.repo.index()?;
        index.clear()?;
        for path in &paths {
            index.add_path(Path::new(path))?;
        }
        index.write()?;
        let tree_id = index.write_tree()?;
        let parent = self.head_commit()?;
        if let Some(parent) = &parent
            && parent.tree_id() == tree_id
        {
            return Ok(None);
        }
        let tree = self.repo.find_tree(tree_id)?;
        let signature = Signature::now("Carrel", "carrel@localhost")?;
        let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
        let oid = self.repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            &change.message(reverts),
            &tree,
            &parents,
        )?;
        Ok(Some(oid.to_string()))
    }

    fn head_commit(&self) -> Result<Option<git2::Commit<'_>>, HistoryError> {
        match self.repo.head() {
            Ok(head) => Ok(Some(head.peel_to_commit()?)),
            Err(e)
                if e.code() == git2::ErrorCode::UnbornBranch
                    || e.code() == git2::ErrorCode::NotFound =>
            {
                Ok(None)
            }
            Err(e) => Err(e.into()),
        }
    }

    /// The text of the metadata file at `path` (relative to the archive root) as last
    /// committed, or `None` if it isn't in the last commit or isn't text.
    ///
    /// # Errors
    ///
    /// A Git error.
    pub fn committed(&self, path: &str) -> Result<Option<String>, HistoryError> {
        let Some(head) = self.head_commit()? else {
            return Ok(None);
        };
        let tree = head.tree()?;
        let Ok(entry) = tree.get_path(Path::new(path)) else {
            return Ok(None);
        };
        let blob = self.repo.find_blob(entry.id())?;
        Ok(std::str::from_utf8(blob.content()).ok().map(str::to_owned))
    }

    /// The history, newest first.
    ///
    /// # Errors
    ///
    /// A Git error.
    pub fn log(&self) -> Result<Vec<Entry>, HistoryError> {
        let mut walk = self.repo.revwalk()?;
        if self.head_commit()?.is_none() {
            return Ok(Vec::new());
        }
        walk.push_head()?;
        walk.set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)?;
        let mut entries = Vec::new();
        for oid in walk {
            let commit = self.repo.find_commit(oid?)?;
            entries.push(entry_from(&commit));
        }
        Ok(entries)
    }

    /// Undoes the change recorded in commit `id` with a new commit that reverses it.
    ///
    /// Items the change moved are moved back first; metadata files are then restored to their
    /// state before the change, merged with any later changes. If later changes touched the
    /// same lines, or an item can't be moved back, nothing is changed and an error says why.
    /// Item contents are not restored here; that is the backup system's job.
    ///
    /// # Errors
    ///
    /// [`HistoryError::UnknownCommit`], [`HistoryError::UndoConflict`],
    /// [`HistoryError::UndoBlocked`], [`HistoryError::Writer`] if `lease` is no longer valid,
    /// or any Git, archive or file error.
    pub fn undo(
        &self,
        archive: &Archive,
        id: &str,
        lease: &WriterLease,
    ) -> Result<String, HistoryError> {
        lease.check()?;
        let oid = Oid::from_str(id).map_err(|_| HistoryError::UnknownCommit(id.to_owned()))?;
        let target = self
            .repo
            .find_commit(oid)
            .map_err(|_| HistoryError::UnknownCommit(id.to_owned()))?;
        let head = self.head_commit()?.ok_or(HistoryError::NotInitialized)?;
        let original = entry_from(&target);
        // Before the first change there was no archive to return to: reverting it would remove
        // every metadata file, including the ones that make the folder an archive.
        if target.parent_count() == 0 {
            return Err(HistoryError::UndoBlocked(
                "the change that started the archive's history can't be undone".into(),
            ));
        }

        let mut reverted = self.repo.revert_commit(&target, &head, 0, None)?;
        if reverted.has_conflicts() {
            let mut paths: Vec<String> = reverted
                .conflicts()?
                .filter_map(Result::ok)
                .filter_map(|c| c.our.or(c.their).or(c.ancestor))
                .map(|e| String::from_utf8_lossy(&e.path).into_owned())
                .collect();
            paths.sort();
            paths.dedup();
            return Err(HistoryError::UndoConflict(paths));
        }

        // Check every move first, so a blocked one changes nothing.
        let moves: Vec<&Move> = original.moves.iter().rev().collect();
        for m in &moves {
            let (now, then) = (self.root.join(&m.to), self.root.join(&m.from));
            if !now.exists() {
                return Err(HistoryError::UndoBlocked(format!(
                    "{} is no longer at {}",
                    m.item, m.to
                )));
            }
            if then.exists() {
                return Err(HistoryError::UndoBlocked(format!("{} is taken", m.from)));
            }
        }
        for m in &moves {
            let (now, then) = (self.root.join(&m.to), self.root.join(&m.from));
            if let Some(parent) = then.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            std::fs::rename(&now, &then).map_err(io(&now))?;
        }

        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.force();
        self.repo
            .checkout_index(Some(&mut reverted), Some(&mut checkout))?;

        let mut change = Change::new(
            original.operation.unwrap_or(Operation::Edit),
            format!("Undo: {}", original.summary),
            lease.info().person,
            lease.info().device,
        );
        change.items.clone_from(&original.items);
        change.moves = original
            .moves
            .iter()
            .rev()
            .map(|m| Move {
                item: m.item,
                from: m.to.clone(),
                to: m.from.clone(),
            })
            .collect();
        // Reopen so collection and card discovery see the restored files.
        let archive = Archive::open(archive.root())?;
        self.commit_with(&archive, &change, Some(oid))?
            .ok_or_else(|| HistoryError::UndoBlocked("the change had nothing to undo".into()))
    }
}

/// Every file under `dir`, relative to `root`, except the folders history never tracks.
fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), HistoryError> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).map_err(io(&current))? {
            let entry = entry.map_err(io(&current))?;
            let path = entry.path();
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let name = entry.file_name().to_string_lossy().into_owned();
            if crate::detect::is_ignored(&name) {
                continue;
            }
            let file_type = entry.file_type().map_err(io(&path))?;
            if file_type.is_dir() {
                let untracked = relative
                    .strip_prefix("_System/")
                    .is_some_and(|rest| UNTRACKED_SYSTEM.contains(&rest));
                if !untracked {
                    stack.push(path);
                }
            } else if file_type.is_file() && !is_lock_file(&relative) {
                out.push(relative);
            }
        }
    }
    Ok(())
}

/// The writer lock and its temporary files change on every lease, so history never tracks them.
fn is_lock_file(relative: &str) -> bool {
    relative
        .strip_prefix(crate::coordinator::LOCK_FILE)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

fn entry_from(commit: &git2::Commit<'_>) -> Entry {
    let message = commit.message().unwrap_or_default();
    let mut entry = Entry {
        id: commit.id().to_string(),
        summary: commit
            .summary()
            .ok()
            .flatten()
            .unwrap_or_default()
            .to_owned(),
        operation: None,
        person: None,
        device: None,
        snapshot: None,
        items: Vec::new(),
        moves: Vec::new(),
        reverts: None,
        time: commit.time().seconds(),
    };
    let parse_id = |v: &str| uuid::Uuid::parse_str(v.trim()).ok().map(Id);
    if let Ok(trailers) = git2::message_trailers_strs(message) {
        for (key, value) in trailers.iter() {
            match key {
                "Carrel-Operation" => entry.operation = Operation::from_name(value.trim()),
                "Carrel-Person" => entry.person = parse_id(value),
                "Carrel-Device" => entry.device = parse_id(value),
                "Carrel-Snapshot" => entry.snapshot = Some(value.trim().to_owned()),
                "Carrel-Items" => entry.items.extend(parse_id(value)),
                "Carrel-Move" => entry.moves.extend(serde_json::from_str(value.trim()).ok()),
                "Carrel-Reverts" => entry.reverts = Some(value.trim().to_owned()),
                _ => {}
            }
        }
    }
    entry
}

/// The history repository's `info/exclude`: ignore everything, because Carrel stages exactly
/// the metadata files it computes. This also keeps plain `git add` from picking up items.
const EXCLUDE: &str = "# Carrel stages exactly the archive's metadata files (FORMAT.md §8.1).\n# Everything is ignored so plain Git commands never pick up item contents.\n*\n";
