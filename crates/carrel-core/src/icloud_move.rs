//! Moving an existing archive into iCloud Drive (`docs/design/sync-and-mobile.md`, "What the
//! owner needs to do"), for the command line, the interactive mode and the Mac app alike:
//!
//! 1. [`plan_move`] checks the destination is new and has room;
//! 2. [`copy_archive`] copies everything, holding the writer lease so nothing changes
//!    meanwhile, then checks every file of the copy against a manifest of the original;
//! 3. [`switch_over`] makes the copy this device's archive and marks the old one as moved.
//!
//! The old copy stays where it was until the owner removes it.

use std::path::{Path, PathBuf};

use crate::archive::Archive;
use crate::coordinator::{LOCK_FILE, WriterLease};
use crate::detect::Rules;
use crate::manifest::Manifest;

/// What [`switch_over`] leaves in the old copy, saying where the archive went.
pub const MOVED_FILE: &str = "_System/moved.json";

/// Room to leave free beyond the archive's size, so the disk doesn't fill to the brim.
const MARGIN: u64 = 256 * 1024 * 1024;

/// Why an archive couldn't be moved.
#[derive(Debug)]
pub enum MoveError {
    /// Something is already at the destination.
    Exists(PathBuf),
    /// The destination is inside the archive.
    Inside(PathBuf),
    /// There isn't room at the destination.
    NoSpace {
        /// The bytes the archive needs, with a margin.
        needed: u64,
        /// The bytes free there.
        free: u64,
    },
    /// The copy doesn't match the original: these files differ or are missing. The copy is
    /// left for a look, and the original is still the archive.
    Mismatch(Vec<String>),
    /// The writer lease was lost while copying, so the original may have changed.
    LeaseLost,
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// This device's preferences couldn't be updated.
    Local(crate::local::LocalError),
}

impl std::fmt::Display for MoveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Exists(p) => write!(f, "{} already exists; choose a new folder", p.display()),
            Self::Inside(p) => write!(f, "{} is inside the archive", p.display()),
            Self::NoSpace { needed, free } => write!(
                f,
                "there isn't room: the archive needs {} and {} is free",
                size(*needed),
                size(*free)
            ),
            Self::Mismatch(files) => write!(
                f,
                "the copy doesn't match the original ({} files differ, such as {}); the original \
                 is still the archive",
                files.len(),
                files.first().map_or("", String::as_str)
            ),
            Self::LeaseLost => f.write_str(
                "another writer took over while copying; the original is still the archive",
            ),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Local(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for MoveError {}

fn io(path: &Path) -> impl FnOnce(std::io::Error) -> MoveError + '_ {
    move |source| MoveError::Io {
        path: path.to_owned(),
        source,
    }
}

/// A size in bytes for people, such as `1.2 GB`.
#[must_use]
pub fn size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let b = bytes as f64;
    match bytes {
        0..1_000 => format!("{bytes} bytes"),
        1_000..1_000_000 => format!("{:.0} KB", b / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} MB", b / 1e6),
        _ => format!("{:.1} GB", b / 1e9),
    }
}

/// iCloud Drive's own folder on this Mac, if it's turned on.
#[must_use]
pub fn icloud_drive() -> Option<PathBuf> {
    let folder = crate::local::home_dir()?.join("Library/Mobile Documents/com~apple~CloudDocs");
    folder.is_dir().then_some(folder)
}

/// Where the archive goes by default: a folder of its name in iCloud Drive.
#[must_use]
pub fn default_destination(archive: &Archive) -> Option<PathBuf> {
    Some(icloud_drive()?.join(archive.root().file_name()?))
}

/// A move, checked and ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MovePlan {
    /// The archive now.
    pub from: PathBuf,
    /// Where it goes.
    pub to: PathBuf,
    /// How many files it has.
    pub files: usize,
    /// Their size in bytes.
    pub bytes: u64,
    /// The bytes free at the destination, where the system says.
    pub free: Option<u64>,
    /// Whether the destination is in a folder a sync service shares, such as iCloud Drive.
    pub synced: bool,
}

/// Checks that `archive` can move to `to`: nothing is there yet, it isn't inside the archive,
/// and there's room.
///
/// # Errors
///
/// [`MoveError::Exists`], [`MoveError::Inside`] or [`MoveError::NoSpace`], or an error
/// reading the archive.
pub fn plan_move(archive: &Archive, to: &Path) -> Result<MovePlan, MoveError> {
    let from = archive.root().to_path_buf();
    if to.exists() {
        return Err(MoveError::Exists(to.to_owned()));
    }
    let to = absolute(to);
    if to.starts_with(&from) || to.starts_with(absolute(&from)) {
        return Err(MoveError::Inside(to));
    }
    let (mut files, mut bytes) = (0, 0);
    walk(&from, &mut |_, meta| {
        files += 1;
        bytes += meta.len();
    })?;
    let free = free_space(&to);
    if let Some(free) = free
        && free < bytes + MARGIN
    {
        return Err(MoveError::NoSpace {
            needed: bytes + MARGIN,
            free,
        });
    }
    Ok(MovePlan {
        synced: crate::coordinator::DiskLock::is_synced_folder(&to),
        from,
        to,
        files,
        bytes,
        free,
    })
}

/// `path` made absolute against the current folder, through its nearest existing folder so
/// symbolic links in it resolve as they do for the archive's root.
fn absolute(path: &Path) -> PathBuf {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let mut existing = path.as_path();
    let mut rest = Vec::new();
    while !existing.exists() {
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
            return path;
        };
        rest.push(name.to_owned());
        existing = parent;
    }
    let mut resolved = existing
        .canonicalize()
        .unwrap_or_else(|_| existing.to_owned());
    resolved.extend(rest.iter().rev());
    resolved
}

/// The bytes free on the volume `path` would be on.
#[must_use]
pub fn free_space(path: &Path) -> Option<u64> {
    let mut existing = path;
    while !existing.exists() {
        existing = existing.parent()?;
    }
    free_space_at(existing)
}

/// The bytes free on the volume holding `path`, from `df`, which every Unix has.
#[cfg(unix)]
fn free_space_at(path: &Path) -> Option<u64> {
    let out = std::process::Command::new("df")
        .arg("-Pk")
        .arg(path)
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    // The second line: file system, blocks, used, available, capacity, mount point.
    let available: u64 = text
        .lines()
        .nth(1)?
        .split_whitespace()
        .nth(3)?
        .parse()
        .ok()?;
    Some(available * 1024)
}

#[cfg(not(unix))]
fn free_space_at(_path: &Path) -> Option<u64> {
    None
}

/// Calls `each` for every file under `root` but the writer lock, with its path relative to
/// `root`.
fn walk(root: &Path, each: &mut dyn FnMut(&Path, &std::fs::Metadata)) -> Result<(), MoveError> {
    let lock = Path::new(LOCK_FILE);
    let mut stack = vec![PathBuf::new()];
    while let Some(relative) = stack.pop() {
        let dir = root.join(&relative);
        for entry in std::fs::read_dir(&dir).map_err(io(&dir))? {
            let entry = entry.map_err(io(&dir))?;
            let rel = relative.join(entry.file_name());
            let meta = std::fs::symlink_metadata(entry.path()).map_err(io(&entry.path()))?;
            if meta.is_dir() {
                stack.push(rel);
            } else if rel != lock {
                each(&rel, &meta);
            }
        }
    }
    Ok(())
}

/// Copies the archive to `plan.to`, holding `lease` so it doesn't change meanwhile, reporting
/// bytes copied and the total to `progress`, then checks the copy against a manifest of the
/// original. Files keep their modification times, so the copy's manifests stay current.
///
/// # Errors
///
/// [`MoveError::Mismatch`] if the copy doesn't match, [`MoveError::LeaseLost`] if another
/// writer took over, or an error copying.
pub fn copy_archive(
    plan: &MovePlan,
    lease: &WriterLease,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), MoveError> {
    // Files iCloud or another service evicted from the original come back first.
    crate::cloud_files::ensure_downloaded(&plan.from).map_err(io(&plan.from))?;
    let everything = Rules {
        markers: Vec::new(),
        regenerable: Vec::new(),
    };
    let item = crate::card::Id::new_v7();
    let original = Manifest::build(&plan.from, item, &everything, None)
        .map_err(|e| MoveError::Mismatch(vec![e.to_string()]))?;
    std::fs::create_dir_all(&plan.to).map_err(io(&plan.to))?;
    let mut copied = 0;
    let mut failed = None;
    walk(&plan.from, &mut |rel, meta| {
        if failed.is_some() {
            return;
        }
        if let Err(e) = copy_one(&plan.from.join(rel), &plan.to.join(rel), meta) {
            failed = Some(e);
            return;
        }
        copied += meta.len();
        progress(copied, plan.bytes);
    })?;
    if let Some(e) = failed {
        return Err(e);
    }
    lease.check().map_err(|_| MoveError::LeaseLost)?;
    let copy = Manifest::build(&plan.to, item, &everything, None)
        .map_err(|e| MoveError::Mismatch(vec![e.to_string()]))?;
    let mut differ: Vec<String> = original
        .entries
        .iter()
        .filter(|e| e.path != LOCK_FILE)
        .filter(|e| {
            !copy
                .entries
                .iter()
                .any(|c| c.path == e.path && c.checksum == e.checksum && c.size == e.size)
        })
        .map(|e| e.path.clone())
        .collect();
    differ.extend(original.not_downloaded);
    if differ.is_empty() {
        Ok(())
    } else {
        Err(MoveError::Mismatch(differ))
    }
}

fn copy_one(from: &Path, to: &Path, meta: &std::fs::Metadata) -> Result<(), MoveError> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(io(parent))?;
    }
    if meta.file_type().is_symlink() {
        let link = std::fs::read_link(from).map_err(io(from))?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(&link, to).map_err(io(to))?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&link, to).map_err(io(to))?;
        return Ok(());
    }
    std::fs::copy(from, to).map_err(io(to))?;
    if let Ok(modified) = meta.modified() {
        let _ = filetime::set_file_mtime(to, filetime::FileTime::from_system_time(modified));
    }
    Ok(())
}

/// What [`MOVED_FILE`] says.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MovedNote {
    /// Where the archive went.
    pub to: PathBuf,
    /// When, as RFC 3339.
    pub moved: String,
}

/// Makes the copy at `plan.to` the archive this device uses, in the preferences kept in
/// `config` (the configuration folder), and leaves a note in the old copy saying where it
/// went. The old copy is kept until the owner removes it.
///
/// # Errors
///
/// [`MoveError::Local`] if the preferences can't be updated, or an error writing the note.
///
/// # Panics
///
/// Never: a note always serializes.
pub fn switch_over(plan: &MovePlan, config: &Path) -> Result<(), MoveError> {
    let mut prefs = crate::local::Prefs::load_from(config).map_err(MoveError::Local)?;
    if prefs.archive.as_deref().is_none_or(|a| a == plan.from) {
        prefs.archive = Some(plan.to.clone());
    }
    prefs.recent.retain(|r| r != &plan.from);
    prefs.note_archive(&plan.to);
    prefs.save_to(config).map_err(MoveError::Local)?;
    let note = MovedNote {
        to: plan.to.clone(),
        moved: jiff::Timestamp::now().to_string(),
    };
    let path = plan.from.join(MOVED_FILE);
    let text = serde_json::to_string_pretty(&note).expect("notes always serialize") + "\n";
    std::fs::write(&path, text).map_err(io(&path))
}

/// Where the archive at `root` was moved to, if it was.
#[must_use]
pub fn moved_to(root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(root.join(MOVED_FILE)).ok()?;
    serde_json::from_str::<MovedNote>(&text).ok().map(|n| n.to)
}
