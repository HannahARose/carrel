//! What a question is about, so it can be shown beside it and answered (#446): the filed
//! items it names and the waiting proposals its answer settles, each with its file or folder
//! when there is one here, and whether it's restricted, whose preview waits until the owner
//! asks for it, as in Browse.

use std::path::PathBuf;

use crate::archive::Archive;
use crate::card::{Id, Kind, Sensitivity};
use crate::card_file::CardFile;
use crate::questions::QuestionEntry;
use crate::review::{ReviewError, ReviewQueue};

/// One thing a question is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionSubject {
    /// The filed item's identifier, or the waiting proposal's.
    pub id: Id,
    /// Whether it's a waiting proposal rather than a filed item.
    pub proposal: bool,
    /// Its card's title, or the proposal's.
    pub title: String,
    /// Its card's summary, when it has one.
    pub summary: Option<String>,
    /// For a filed item, its card's path relative to the archive root.
    pub card: Option<String>,
    /// Its file, folder or bundle, as an absolute path, when it's on this device: `None` for
    /// a card-only item, or a file that isn't there.
    pub path: Option<PathBuf>,
    /// Its kind.
    pub kind: Kind,
    /// Whether it's restricted, so its preview waits until it's asked for.
    pub restricted: bool,
}

/// What `question` is about: the filed items it names, then the waiting proposals its answer
/// settles, each once. Items or proposals that have gone since it was asked are left out.
///
/// # Errors
///
/// [`ReviewError`] if the review queue can't be read; an archive that can't be listed gives
/// no filed items.
pub fn subjects(
    archive: &Archive,
    question: &QuestionEntry,
) -> Result<Vec<QuestionSubject>, ReviewError> {
    let root = archive.root();
    let mut found = Vec::new();
    if !question.about.is_empty() {
        let items = archive.items().unwrap_or_default();
        for id in &question.about {
            let Some(item) = items.iter().find(|i| i.id == *id) else {
                continue;
            };
            let Some(file) = std::fs::read_to_string(root.join(&item.card))
                .ok()
                .and_then(|t| CardFile::parse(&t).ok())
            else {
                continue;
            };
            let card = file.card();
            found.push(QuestionSubject {
                id: item.id,
                proposal: false,
                title: card.title.clone(),
                summary: card.summary.clone(),
                card: Some(item.card.clone()),
                path: item
                    .item
                    .as_deref()
                    .map(|p| root.join(p))
                    .filter(|p| p.exists()),
                kind: item.kind,
                restricted: card.sensitivity == Sensitivity::Restricted,
            });
        }
    }
    if !question.items.is_empty() {
        let (proposals, _) = ReviewQueue::of(archive).list()?;
        for id in &question.items {
            let Some(entry) = proposals.iter().find(|e| e.id == *id) else {
                continue;
            };
            found.push(QuestionSubject {
                id: entry.id,
                proposal: true,
                title: entry.card.title.clone(),
                summary: entry.card.summary.clone(),
                card: None,
                path: Some(entry.source.path.clone()).filter(|p| p.exists()),
                kind: entry.kind,
                restricted: entry.card.sensitivity == Sensitivity::Restricted,
            });
        }
    }
    Ok(found)
}
