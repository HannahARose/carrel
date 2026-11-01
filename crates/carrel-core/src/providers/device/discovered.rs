//! What the provider programs on this device offered, kept for the life of the process, so
//! that building a registry doesn't start and stop every program each time (#333).
//!
//! The first registry starts the programs together and keeps what each says it offers.
//! Later ones, while the programs and the models folder are unchanged, are made from that,
//! and each program starts only when it's asked to do something. Only one thread asks the
//! programs at a time; another that needs them meanwhile waits and uses what it found.

use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};
use std::time::SystemTime;

use super::super::ProviderError;
use super::super::process::{Found, Introduced, ProcessProvider};
use super::Failed;

/// How many sets of programs are remembered, as when tests use several folders.
const KEPT: usize = 8;

/// What was found, most recent last.
static FOUND: Mutex<Vec<Asked>> = Mutex::new(Vec::new());

/// What a set of programs offered, and what it depended on.
struct Asked {
    stamp: Stamp,
    answers: Vec<(Found, Result<Introduced, String>)>,
}

/// What a set of programs' offers depend on: the programs themselves, and the models they
/// can find.
#[derive(Debug, PartialEq, Eq)]
struct Stamp {
    programs: Vec<FileStamp>,
    models: Vec<FileStamp>,
}

/// A file or folder, as far as telling that it changed.
#[derive(Debug, PartialEq, Eq)]
struct FileStamp {
    path: PathBuf,
    modified: Option<SystemTime>,
    len: u64,
}

impl FileStamp {
    fn of(path: PathBuf) -> Self {
        let metadata = std::fs::metadata(&path).ok();
        Self {
            modified: metadata.as_ref().and_then(|m| m.modified().ok()),
            len: metadata.map_or(0, |m| m.len()),
            path,
        }
    }
}

impl Stamp {
    fn of(found: &[Found]) -> Self {
        Self {
            programs: found
                .iter()
                .map(|f| FileStamp::of(f.path.clone()))
                .collect(),
            models: crate::local::models_dir().map_or_else(Vec::new, |dir| models(&dir)),
        }
    }
}

/// Everything in the models folder and the folders in it, such as MLX models, which are
/// folders of files.
fn models(dir: &std::path::Path) -> Vec<FileStamp> {
    let mut stamps = vec![FileStamp::of(dir.to_owned())];
    let Ok(entries) = std::fs::read_dir(dir) else {
        return stamps;
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if path.is_dir() {
            stamps.extend(models(&path));
        } else {
            stamps.push(FileStamp::of(path));
        }
    }
    stamps
}

/// The providers `found`: started, if they haven't been asked what they offer since they or
/// the models changed, otherwise made from what they said.
pub(super) fn providers(found: Vec<Found>) -> Vec<Result<ProcessProvider, Failed>> {
    let stamp = Stamp::of(&found);
    // Held while the programs are asked, so another thread waits for these answers rather
    // than starting every program again at the same moment.
    let mut asked = FOUND.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(known) = asked.iter().find(|a| a.stamp == stamp) {
        return known
            .answers
            .iter()
            .map(|(found, answer)| match answer {
                Ok(introduced) => Ok(ProcessProvider::introduced(
                    found.clone(),
                    introduced.clone(),
                )),
                Err(why) => Err(Failed {
                    found: found.clone(),
                    error: ProviderError::Protocol(why.clone()),
                }),
            })
            .collect();
    }
    let started = start_together(found);
    let answers = started
        .iter()
        .map(|(found, result)| {
            let answer = result
                .as_ref()
                .map(|p| p.introduction().clone())
                .map_err(ToString::to_string);
            (found.clone(), answer)
        })
        .collect();
    if asked.len() == KEPT {
        asked.remove(0);
    }
    asked.push(Asked { stamp, answers });
    drop(asked);
    started
        .into_iter()
        .map(|(found, result)| result.map_err(|error| Failed { found, error }))
        .collect()
}

/// Starts every program in `found` at once, so the slowest decides how long it takes.
fn start_together(found: Vec<Found>) -> Vec<(Found, Result<ProcessProvider, ProviderError>)> {
    std::thread::scope(|scope| {
        let starting: Vec<_> = found
            .iter()
            .map(|f| scope.spawn(|| ProcessProvider::start(f)))
            .collect();
        found
            .iter()
            .zip(starting)
            .map(|(f, thread)| {
                let result = thread.join().unwrap_or_else(|_| {
                    Err(ProviderError::Protocol(format!(
                        "{}: starting it failed",
                        f.name
                    )))
                });
                (f.clone(), result)
            })
            .collect()
    })
}

/// Forgets everything found, so the next registry asks the programs again.
pub(super) fn forget() {
    FOUND.lock().unwrap_or_else(PoisonError::into_inner).clear();
}
