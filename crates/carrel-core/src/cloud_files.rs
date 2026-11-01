//! iCloud Drive's on-demand files (`docs/design/sync-and-mobile.md`, "iCloud Drive's files").
//!
//! iCloud Drive may evict a file's contents to save space, leaving a placeholder: on current
//! macOS a "dataless" file with the file's own name and size, and on older systems, iCloud for
//! Windows and copies made from them, a hidden `.<name>.icloud` file in its place. Carrel never
//! treats an evicted file as missing or changed: manifests keep what they last knew of it
//! ([`crate::manifest::Manifest::build`]), and an item's contents are downloaded only when they
//! are needed, by opening, previewing, checking out, drafting or snapshotting
//! ([`ensure_downloaded`]).
//!
//! Whether a file is downloaded, and downloading it, go through an [`OnDemandFiles`] provider.
//! On Apple platforms the app registers one ([`set_provider`]) that asks `NSURL`'s
//! ubiquitous-item resource values and downloads through file coordination. Elsewhere, and in
//! the command line, [`Placeholders`] stands in: it recognizes both kinds of placeholder and
//! downloads what the system lets it.
//!
//! A collection whose design document says `sync: {keep_downloaded: true}` keeps its items on
//! every device ([`keeps_downloaded`], [`keep_downloaded_pass`]); the conflicted copies iCloud
//! leaves are found by [`conflicts()`].

pub mod conflicts;
pub mod keep;

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

pub use conflicts::{ConflictedCopy, Keep, conflicts, resolve};
pub use keep::{
    KeptPass, keep_downloaded_pass, keeps_downloaded, own_setting, set_keep_downloaded,
};

/// Why a conflicted copy couldn't be resolved, or `sync.keep_downloaded` set.
#[derive(Debug)]
pub enum SyncError {
    /// The request doesn't fit the archive, such as a collection that isn't there.
    Invalid(String),
    /// A file operation failed.
    Io {
        /// The path concerned.
        path: PathBuf,
        /// The underlying error.
        source: std::io::Error,
    },
    /// The archive couldn't be read.
    Archive(crate::archive::ArchiveError),
    /// History couldn't record the change.
    History(crate::history::HistoryError),
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Io { path, source } => write!(f, "{}: {source}", path.display()),
            Self::Archive(e) => e.fmt(f),
            Self::History(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for SyncError {}

impl From<crate::archive::ArchiveError> for SyncError {
    fn from(e: crate::archive::ArchiveError) -> Self {
        Self::Archive(e)
    }
}

impl From<crate::history::HistoryError> for SyncError {
    fn from(e: crate::history::HistoryError) -> Self {
        Self::History(e)
    }
}

/// Wraps an I/O error with the path it happened on.
pub(crate) fn io(path: &Path) -> impl FnOnce(std::io::Error) -> SyncError + '_ {
    move |source| SyncError::Io {
        path: path.to_owned(),
        source,
    }
}

/// The suffix of the placeholder iCloud Drive leaves for an evicted file on older systems.
pub const PLACEHOLDER_SUFFIX: &str = ".icloud";

/// Whether a file's contents are on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    /// Its contents are here.
    Downloaded,
    /// Only a placeholder is here; the contents are in iCloud.
    NotDownloaded,
    /// It is being downloaded.
    Downloading,
    /// Neither the file nor a placeholder for it is here.
    Missing,
}

impl DownloadState {
    /// Whether the contents can't be read on this device yet.
    #[must_use]
    pub fn is_evicted(self) -> bool {
        matches!(self, Self::NotDownloaded | Self::Downloading)
    }
}

/// Finds out whether files are downloaded, and downloads them. Each path is the file's own
/// path, never its placeholder's.
pub trait OnDemandFiles: Send + Sync {
    /// Whether the file at `path` is downloaded.
    fn state(&self, path: &Path) -> DownloadState;

    /// Downloads the file at `path`, waiting until its contents are here.
    ///
    /// # Errors
    ///
    /// Any error downloading it, such as being offline.
    fn download(&self, path: &Path) -> std::io::Result<()>;
}

/// The provider used without one from the platform: recognizes `.<name>.icloud` placeholders
/// and, on macOS, dataless files. A dataless file is downloaded by reading it, which the
/// system answers by fetching its contents; a `.icloud` placeholder by `brctl download` on
/// macOS, and nowhere else, where the platform's sync client has to fetch it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Placeholders;

/// macOS's flag for a file whose contents aren't on the device (`SF_DATALESS`).
#[cfg(target_os = "macos")]
const SF_DATALESS: u32 = 0x4000_0000;

/// Whether `meta` describes a dataless file: on macOS one whose contents iCloud evicted.
#[must_use]
pub fn is_dataless(meta: &std::fs::Metadata) -> bool {
    #[cfg(target_os = "macos")]
    {
        use std::os::macos::fs::MetadataExt as _;
        meta.st_flags() & SF_DATALESS != 0
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = meta;
        false
    }
}

impl OnDemandFiles for Placeholders {
    fn state(&self, path: &Path) -> DownloadState {
        match std::fs::symlink_metadata(path) {
            Ok(meta) if meta.is_file() && is_dataless(&meta) => DownloadState::NotDownloaded,
            Ok(_) => DownloadState::Downloaded,
            Err(_) if placeholder_path(path).is_some_and(|p| p.is_file()) => {
                DownloadState::NotDownloaded
            }
            Err(_) => DownloadState::Missing,
        }
    }

    fn download(&self, path: &Path) -> std::io::Result<()> {
        match self.state(path) {
            DownloadState::Downloaded => Ok(()),
            DownloadState::Missing => Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("{} isn't here", path.display()),
            )),
            _ if path.exists() => {
                // A dataless file: reading it makes the system fetch its contents.
                std::io::copy(&mut std::fs::File::open(path)?, &mut std::io::sink()).map(|_| ())
            }
            _ => download_placeholder(path),
        }
    }
}

/// Asks iCloud Drive to download the file behind a `.icloud` placeholder and waits for it.
fn download_placeholder(path: &Path) -> std::io::Result<()> {
    let not_here = || {
        std::io::Error::other(format!(
            "{} isn't downloaded; open it in iCloud Drive to download it",
            path.display()
        ))
    };
    if !cfg!(target_os = "macos") {
        return Err(not_here());
    }
    let status = std::process::Command::new("brctl")
        .arg("download")
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|_| not_here())?;
    if !status.success() {
        return Err(not_here());
    }
    // `brctl` asks for the download; the file appears when it's done.
    for _ in 0..600 {
        if path.exists() {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        format!("{} is still downloading", path.display()),
    ))
}

/// The provider the platform registered, if any.
static PROVIDER: RwLock<Option<Arc<dyn OnDemandFiles>>> = RwLock::new(None);

thread_local! {
    /// A provider for this thread only, for tests ([`with_provider`]).
    static OVERRIDE: std::cell::RefCell<Option<Arc<dyn OnDemandFiles>>> =
        const { std::cell::RefCell::new(None) };
}

/// Uses `provider` from now on, in place of [`Placeholders`]: the Mac, iPad and iPhone apps
/// register one that uses file coordination.
pub fn set_provider(provider: Arc<dyn OnDemandFiles>) {
    *PROVIDER
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(provider);
}

/// The provider in use: [`with_provider`]'s on this thread, else [`set_provider`]'s, else
/// [`Placeholders`].
#[must_use]
pub fn provider() -> Arc<dyn OnDemandFiles> {
    if let Some(provider) = OVERRIDE.with(|o| o.borrow().clone()) {
        return provider;
    }
    PROVIDER
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .unwrap_or_else(|| Arc::new(Placeholders))
}

/// Runs `f` with `provider` in use on this thread only, as tests do with a fake.
pub fn with_provider<T>(provider: Arc<dyn OnDemandFiles>, f: impl FnOnce() -> T) -> T {
    let previous = OVERRIDE.with(|o| o.borrow_mut().replace(provider));
    let result = f();
    OVERRIDE.with(|o| *o.borrow_mut() = previous);
    result
}

/// The name of the file a placeholder stands for: `report.pdf` for `.report.pdf.icloud`.
#[must_use]
pub fn placeholder_for(name: &str) -> Option<&str> {
    name.strip_prefix('.')?
        .strip_suffix(PLACEHOLDER_SUFFIX)
        .filter(|n| !n.is_empty())
}

/// Where the `.icloud` placeholder for the file at `path` would be.
#[must_use]
pub fn placeholder_path(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    Some(path.with_file_name(format!(".{name}{PLACEHOLDER_SUFFIX}")))
}

/// Downloads the file at `path` if only a `.icloud` placeholder is there, as for a card or a
/// design document, which Carrel reads whenever it lists the archive. A dataless file needs
/// nothing: reading it downloads it. Returns whether the file is there now.
#[must_use]
pub fn fetch_if_placeholder(path: &Path) -> bool {
    if path.exists() {
        return true;
    }
    if !placeholder_path(path).is_some_and(|p| p.is_file()) {
        return false;
    }
    provider().download(path).is_ok() && path.exists()
}

/// Downloads the file or folder at `path` (everything in a folder) if any of it is evicted,
/// before its contents are read: opening, previewing, checking out, drafting, snapshotting.
///
/// # Errors
///
/// The first file that couldn't be downloaded, and why.
pub fn ensure_downloaded(path: &Path) -> std::io::Result<()> {
    let provider = provider();
    for file in not_downloaded_in(path, provider.as_ref()) {
        provider.download(&file)?;
    }
    Ok(())
}

/// The evicted files at `path`, a file or a folder searched through, by their own paths.
#[must_use]
pub fn not_downloaded_in(path: &Path, provider: &dyn OnDemandFiles) -> Vec<PathBuf> {
    let mut found = Vec::new();
    if !path.is_dir() {
        if provider.state(path).is_evicted() {
            found.push(path.to_path_buf());
        }
        return found;
    }
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let file = entry.path();
            if let Some(real) = placeholder_for(&name) {
                let real = dir.join(real);
                if !real.exists() {
                    found.push(real);
                }
                continue;
            }
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                if !is_local_only(&name) {
                    stack.push(file);
                }
            } else if provider.state(&file).is_evicted() {
                found.push(file);
            }
        }
    }
    found.sort();
    found
}

/// Folders iCloud never syncs (`*.nosync`, such as `cache.nosync`) and history's own git
/// folder, which Carrel reads through git.
fn is_local_only(name: &str) -> bool {
    name.ends_with(".nosync") || name == ".git"
}

/// How much of an archive isn't downloaded on this device.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NotDownloaded {
    /// The evicted files, relative to the archive root with `/` separators, sorted.
    pub files: Vec<String>,
    /// Their size in bytes, where the placeholder says (a dataless file does; a `.icloud`
    /// placeholder doesn't, and counts nothing).
    pub bytes: u64,
}

/// The evicted files in the archive at `root`, by the provider in use.
#[must_use]
pub fn not_downloaded(root: &Path) -> NotDownloaded {
    let provider = provider();
    let found = not_downloaded_in(root, provider.as_ref());
    let bytes = found
        .iter()
        .filter_map(|f| std::fs::metadata(f).ok())
        .map(|m| m.len())
        .sum();
    NotDownloaded {
        files: found.iter().map(|f| relative(root, f)).collect(),
        bytes,
    }
}

/// `path` relative to `root`, with `/` separators.
pub(crate) fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_names_map_to_the_file_they_stand_for() {
        assert_eq!(placeholder_for(".report.pdf.icloud"), Some("report.pdf"));
        assert_eq!(placeholder_for(".icloud"), None);
        assert_eq!(placeholder_for("report.pdf.icloud"), None);
        assert_eq!(
            placeholder_path(Path::new("/a/report.pdf")).unwrap(),
            Path::new("/a/.report.pdf.icloud")
        );
    }

    #[test]
    fn placeholders_are_evicted_and_never_missing() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("here.txt"), "x").unwrap();
        std::fs::write(dir.path().join(".away.pdf.icloud"), "plist").unwrap();
        std::fs::create_dir(dir.path().join("cache.nosync")).unwrap();
        std::fs::write(dir.path().join("cache.nosync/.skip.icloud"), "").unwrap();
        let p = Placeholders;
        assert_eq!(
            p.state(&dir.path().join("here.txt")),
            DownloadState::Downloaded
        );
        assert_eq!(
            p.state(&dir.path().join("away.pdf")),
            DownloadState::NotDownloaded
        );
        assert_eq!(p.state(&dir.path().join("gone")), DownloadState::Missing);
        let found = not_downloaded(dir.path());
        assert_eq!(found.files, ["away.pdf"]);
    }
}
