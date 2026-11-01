//! Noticing changes by looking: a [`Poller`] remembers each file's size and modification time
//! in the folders it watches, and says which paths changed since it last looked.
//!
//! In an archive it looks only where a change means something to the agent
//! ([`super::plan::Plan::classify`]): the filed items, the intake queue, the desk files and
//! the checkout manifests. It leaves out the rest of `_System`, where the agent's own writes
//! (the index, the text cache, the history repository) would wake it for nothing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// How often the agent looks for changes, at least: more seldom when looking takes long, so
/// looking never takes more than a tenth of the time.
pub const POLL: Duration = Duration::from_secs(5);

/// What a file looked like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seen {
    modified: Option<SystemTime>,
    len: u64,
    folder: bool,
}

/// Watches folders by looking at them.
#[derive(Debug)]
pub struct Poller {
    /// The archive's root, where only part of `_System` is looked at.
    archive: PathBuf,
    /// Working copies outside the archive.
    outside: Vec<PathBuf>,
    seen: HashMap<PathBuf, Seen>,
    /// How long looking took last time.
    took: Duration,
}

/// The parts of `_System` worth looking at.
const SYSTEM_PARTS: [&str; 3] = ["queue/intake", "devices", "manifests"];

impl Poller {
    /// A poller for the archive at `archive` and the working copies `working_copies`, having
    /// looked once, so the first [`Poller::changes`] reports only what changes after this.
    #[must_use]
    pub fn new(archive: &Path, working_copies: &[PathBuf]) -> Self {
        let mut poller = Self {
            archive: archive.to_owned(),
            outside: Vec::new(),
            seen: HashMap::new(),
            took: Duration::ZERO,
        };
        poller.watch(working_copies);
        poller
    }

    /// Watches the working copies `working_copies` from now on, besides the archive, looking
    /// again without reporting what's there as changed.
    pub fn watch(&mut self, working_copies: &[PathBuf]) {
        self.outside = working_copies
            .iter()
            .filter(|w| !w.starts_with(&self.archive))
            .cloned()
            .collect();
        self.seen = self.look();
    }

    /// How long to wait before looking again.
    #[must_use]
    pub fn interval(&self) -> Duration {
        POLL.max(self.took * 10)
    }

    /// The paths added, removed or changed since the last look, in no particular order.
    pub fn changes(&mut self) -> Vec<PathBuf> {
        let now = self.look();
        let mut changed: Vec<PathBuf> = now
            .iter()
            .filter(|(path, seen)| {
                self.seen
                    .get(*path)
                    // A folder's own time changes with what's in it, which is reported anyway.
                    .is_none_or(|before| before != *seen && !(before.folder && seen.folder))
            })
            .map(|(path, _)| path.clone())
            .collect();
        changed.extend(
            self.seen
                .keys()
                .filter(|path| !now.contains_key(*path))
                .cloned(),
        );
        self.seen = now;
        changed
    }

    fn look(&mut self) -> HashMap<PathBuf, Seen> {
        let started = Instant::now();
        let mut seen = HashMap::new();
        let system = self.archive.join("_System");
        walk(&self.archive, &mut seen, &|path| path != system);
        for part in SYSTEM_PARTS {
            walk(&system.join(part), &mut seen, &|_| true);
        }
        for folder in &self.outside {
            walk(folder, &mut seen, &|_| true);
        }
        self.took = started.elapsed();
        seen
    }
}

/// Records `path` and, for a folder, everything in it that `enter` lets in. Links aren't
/// followed; what can't be read is left out.
fn walk(path: &Path, seen: &mut HashMap<PathBuf, Seen>, enter: &dyn Fn(&Path) -> bool) {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return;
    };
    let folder = meta.is_dir();
    seen.insert(
        path.to_owned(),
        Seen {
            modified: meta.modified().ok(),
            len: if folder { 0 } else { meta.len() },
            folder,
        },
    );
    if !folder {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        let child = entry.path();
        if enter(&child) {
            walk(&child, seen, enter);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
        paths.sort();
        paths
    }

    #[test]
    fn it_notices_files_added_changed_and_removed_where_they_matter() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("Archive");
        let intake = archive.join(crate::intake::INTAKE_DIR);
        std::fs::create_dir_all(&intake).unwrap();
        std::fs::create_dir_all(archive.join("_System/index")).unwrap();
        std::fs::create_dir_all(archive.join("Reading")).unwrap();
        std::fs::write(archive.join("Reading/a.pdf"), "a").unwrap();
        let outside = dir.path().join("Thesis");
        std::fs::create_dir_all(&outside).unwrap();

        let mut poller = Poller::new(&archive, std::slice::from_ref(&outside));
        assert_eq!(poller.changes(), Vec::<PathBuf>::new(), "nothing new yet");

        std::fs::write(intake.join("x.json"), "{}").unwrap();
        std::fs::write(archive.join("_System/index/vectors"), "v").unwrap();
        std::fs::write(archive.join("Reading/a.pdf"), "longer").unwrap();
        std::fs::write(outside.join("chapter.tex"), "text").unwrap();
        assert_eq!(
            sorted(poller.changes()),
            sorted(vec![
                intake.join("x.json"),
                archive.join("Reading/a.pdf"),
                outside.join("chapter.tex"),
            ]),
            "the index isn't looked at"
        );

        std::fs::remove_file(intake.join("x.json")).unwrap();
        assert_eq!(poller.changes(), [intake.join("x.json")]);
        assert_eq!(poller.changes(), Vec::<PathBuf>::new());
        assert!(poller.interval() >= POLL);
    }

    #[test]
    fn watching_other_working_copies_reports_nothing_until_they_change() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("Archive");
        std::fs::create_dir_all(&archive).unwrap();
        let outside = dir.path().join("Desk item");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("notes.md"), "notes").unwrap();
        let mut poller = Poller::new(&archive, &[]);
        poller.watch(std::slice::from_ref(&outside));
        assert_eq!(poller.changes(), Vec::<PathBuf>::new());
        std::fs::write(outside.join("notes.md"), "more notes").unwrap();
        assert_eq!(poller.changes(), [outside.join("notes.md")]);
    }
}
