//! What the provider programs on this device offered, kept for the life of the process, so
//! that building a registry doesn't start and stop every program each time (#333).
//!
//! The first registry starts the programs together and keeps what each says it offers.
//! Later ones, while the programs and the models folder are unchanged, are made from that,
//! and each program starts only when it's asked to do something. Only one thread asks the
//! programs at a time; another that needs them meanwhile waits and uses what it found.
//!
//! A program that couldn't start, or offered something it couldn't do yet (Apple's model
//! still downloading), is asked again on its own after [`RETRY`], so a passing failure isn't
//! remembered for the rest of the process. Forgetting never waits for programs being asked.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use super::super::ProviderError;
use super::super::process::{Found, Introduced, ProcessProvider};
use super::Failed;

/// How many sets of programs are remembered, as when tests use several folders.
const KEPT: usize = 8;

/// How long before a program that couldn't start, or offered something unavailable, is asked
/// again, unless [`retry_after`] changed it.
pub const RETRY: Duration = Duration::from_mins(1);

/// [`RETRY`] as it is now, in milliseconds.
#[allow(clippy::cast_possible_truncation)] // a minute, in milliseconds
static RETRY_MS: AtomicU64 = AtomicU64::new(RETRY.as_millis() as u64);

/// Sets how long before a program that couldn't start, or offered something unavailable, is
/// asked again. For tests, which can't wait [`RETRY`].
#[doc(hidden)]
pub fn retry_after(wait: Duration) {
    RETRY_MS.store(
        u64::try_from(wait.as_millis()).unwrap_or(u64::MAX),
        Ordering::SeqCst,
    );
}

/// What was found, most recent last.
static FOUND: Mutex<Vec<Asked>> = Mutex::new(Vec::new());

/// Bumped by [`forget`], so what was found before is passed over without taking the lock,
/// which is held while programs are asked.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// What a set of programs offered, and what it depended on.
struct Asked {
    stamp: Stamp,
    generation: u64,
    answers: Vec<Answer>,
}

/// What one program said, and when.
struct Answer {
    found: Found,
    said: Result<Introduced, ProviderError>,
    when: Instant,
}

impl Answer {
    /// Whether it's worth asking again: it couldn't start, or something it offers can't be
    /// used yet.
    fn unsettled(&self) -> bool {
        self.said
            .as_ref()
            .map_or(true, |i| i.offers.iter().any(|o| !o.available))
    }
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
pub(super) fn providers(found: &[Found]) -> Vec<Result<ProcessProvider, Failed>> {
    let stamp = Stamp::of(found);
    let generation = GENERATION.load(Ordering::SeqCst);
    // Held while the programs are asked, so another thread waits for these answers rather
    // than starting every program again at the same moment.
    let mut asked = FOUND.lock().unwrap_or_else(PoisonError::into_inner);
    asked.retain(|a| a.generation == generation);
    if let Some(known) = asked.iter_mut().find(|a| a.stamp == stamp) {
        let again: Vec<Found> = known
            .answers
            .iter()
            .filter(|a| {
                a.unsettled()
                    && a.when.elapsed() >= Duration::from_millis(RETRY_MS.load(Ordering::SeqCst))
            })
            .map(|a| a.found.clone())
            .collect();
        for (found, result) in start_together(&again) {
            if let Some(answer) = known.answers.iter_mut().find(|a| a.found == found) {
                answer.said = result.map(|p| p.introduction().clone());
                answer.when = Instant::now();
            }
        }
        return known
            .answers
            .iter()
            .map(|answer| match &answer.said {
                Ok(introduced) => Ok(ProcessProvider::introduced(
                    answer.found.clone(),
                    introduced.clone(),
                )),
                Err(error) => Err(Failed {
                    found: answer.found.clone(),
                    error: error.clone(),
                }),
            })
            .collect();
    }
    let started = start_together(found);
    let answers = started
        .iter()
        .map(|(found, result)| Answer {
            found: found.clone(),
            said: result
                .as_ref()
                .map(|p| p.introduction().clone())
                .map_err(Clone::clone),
            when: Instant::now(),
        })
        .collect();
    if asked.len() == KEPT {
        asked.remove(0);
    }
    asked.push(Asked {
        stamp,
        generation,
        answers,
    });
    drop(asked);
    started
        .into_iter()
        .map(|(found, result)| result.map_err(|error| Failed { found, error }))
        .collect()
}

/// Starts every program in `found` at once, so the slowest decides how long it takes.
fn start_together(found: &[Found]) -> Vec<(Found, Result<ProcessProvider, ProviderError>)> {
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

/// Forgets everything found, so the next registry asks the programs again. It never waits,
/// even while another thread is asking them.
pub(super) fn forget() {
    GENERATION.fetch_add(1, Ordering::SeqCst);
}
