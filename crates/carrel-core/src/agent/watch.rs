//! Noticing changes: through the operating system's file events where it can (inotify on
//! Linux, `FSEvents` on macOS, `ReadDirectoryChangesW` on Windows, by way of `notify`), and by
//! looking ([`Poller`]) where it can't, as on some network drives, or when asked to.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use notify::{RecursiveMode, Watcher as _};

use super::poll::Poller;

/// How often events that have arrived are picked up.
pub const EVENTS: Duration = Duration::from_secs(1);

/// How changes are noticed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum How {
    /// The operating system's file events.
    Events,
    /// Looking every few seconds.
    Polling,
}

/// Watches an archive and the working copies outside it.
pub enum Watcher {
    /// Through file events.
    Events {
        /// The watcher; dropping it stops the events.
        watcher: notify::RecommendedWatcher,
        /// The paths changed, as events arrive.
        paths: Receiver<PathBuf>,
        /// The archive's root.
        archive: PathBuf,
        /// The working copies outside the archive being watched.
        outside: Vec<PathBuf>,
    },
    /// By looking.
    Polling(Poller),
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Events {
                archive, outside, ..
            } => f
                .debug_struct("Events")
                .field("archive", archive)
                .field("outside", outside)
                .finish_non_exhaustive(),
            Self::Polling(poller) => f.debug_tuple("Polling").field(poller).finish(),
        }
    }
}

impl Watcher {
    /// Watches the archive at `archive` and the working copies `working_copies`, through file
    /// events unless `polling`, or if they can't be had.
    #[must_use]
    pub fn new(archive: &Path, working_copies: &[PathBuf], polling: bool) -> Self {
        if !polling && let Some(watcher) = Self::events(archive, working_copies) {
            return watcher;
        }
        Self::Polling(Poller::new(archive, working_copies))
    }

    fn events(archive: &Path, working_copies: &[PathBuf]) -> Option<Self> {
        let (send, paths) = channel();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                // Reading a file is reported too, on Linux; the agent reads cards all the time.
                if let Ok(event) = event
                    && !matches!(event.kind, notify::EventKind::Access(_))
                {
                    for path in event.paths {
                        let _ = send.send(path);
                    }
                }
            })
            .ok()?;
        watcher.watch(archive, RecursiveMode::Recursive).ok()?;
        let mut watcher = Self::Events {
            watcher,
            paths,
            archive: archive.to_owned(),
            outside: Vec::new(),
        };
        watcher.watch(working_copies);
        Some(watcher)
    }

    /// How changes are noticed.
    #[must_use]
    pub fn how(&self) -> How {
        match self {
            Self::Events { .. } => How::Events,
            Self::Polling(_) => How::Polling,
        }
    }

    /// Watches the working copies `working_copies` from now on, besides the archive.
    pub fn watch(&mut self, working_copies: &[PathBuf]) {
        match self {
            Self::Events {
                watcher,
                archive,
                outside,
                ..
            } => {
                let wanted: Vec<PathBuf> = working_copies
                    .iter()
                    .filter(|w| !w.starts_with(&*archive))
                    .cloned()
                    .collect();
                for gone in outside.iter().filter(|o| !wanted.contains(o)) {
                    let _ = watcher.unwatch(gone);
                }
                let mut watched = Vec::new();
                for folder in wanted {
                    // A working copy that isn't there yet, or can't be watched, is left out;
                    // reading what's active again tries it once more.
                    if outside.contains(&folder)
                        || watcher.watch(&folder, RecursiveMode::Recursive).is_ok()
                    {
                        watched.push(folder);
                    }
                }
                *outside = watched;
            }
            Self::Polling(poller) => poller.watch(working_copies),
        }
    }

    /// How long to wait before asking for changes again.
    #[must_use]
    pub fn interval(&self) -> Duration {
        match self {
            Self::Events { .. } => EVENTS,
            Self::Polling(poller) => poller.interval(),
        }
    }

    /// The paths changed since last asked.
    pub fn changes(&mut self) -> Vec<PathBuf> {
        match self {
            Self::Events { paths, .. } => paths.try_iter().collect(),
            Self::Polling(poller) => poller.changes(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    /// Waits for `watcher` to report `path`, for up to ten seconds.
    fn reported(watcher: &mut Watcher, path: &Path) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(10) {
            if watcher
                .changes()
                .iter()
                .any(|p| p.ends_with(path.file_name().unwrap()))
            {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn file_events_report_changes_in_the_archive_and_working_copies_outside_it() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("Archive");
        std::fs::create_dir_all(archive.join("Reading")).unwrap();
        let outside = dir.path().join("Thesis");
        std::fs::create_dir_all(&outside).unwrap();
        let mut watcher = Watcher::new(&archive, std::slice::from_ref(&outside), false);
        assert_eq!(watcher.how(), How::Events);
        assert_eq!(watcher.interval(), EVENTS);

        let filed = archive.join("Reading/paper.pdf");
        std::fs::write(&filed, "pdf").unwrap();
        assert!(reported(&mut watcher, &filed));
        let chapter = outside.join("chapter.tex");
        std::fs::write(&chapter, "words").unwrap();
        assert!(reported(&mut watcher, &chapter));

        watcher.watch(&[]);
        let _ = watcher.changes();
        std::fs::write(outside.join("later.tex"), "more").unwrap();
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !watcher.changes().iter().any(|p| p.starts_with(&outside)),
            "a working copy no longer active isn't watched"
        );
    }

    #[test]
    fn polling_is_used_when_asked() {
        let dir = tempfile::tempdir().unwrap();
        let mut watcher = Watcher::new(dir.path(), &[], true);
        assert_eq!(watcher.how(), How::Polling);
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        assert_eq!(watcher.changes(), [dir.path().join("a.txt")]);
    }
}
