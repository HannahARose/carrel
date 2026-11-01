//! What a question is about, to show beside it (#446): each filed item or waiting proposal
//! with its file, for Quick Look, through [`carrel_core::questions::subjects`].

use carrel_core::questions::QuestionQueue;
use carrel_core::questions::subjects::subjects;

use crate::{ArchiveHandle, CarrelError, ItemKind, text};

/// One thing a question is about.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct QuestionSubject {
    /// The filed item's identifier, or the waiting proposal's.
    pub id: String,
    /// Whether it's a waiting proposal rather than a filed item.
    pub proposal: bool,
    /// Its title.
    pub title: String,
    /// Its summary, when it has one.
    pub summary: Option<String>,
    /// For a filed item, its card's path relative to the archive root.
    pub card: Option<String>,
    /// Its file, folder or bundle, as an absolute path, when it's on this device.
    pub file: Option<String>,
    /// Its kind.
    pub kind: ItemKind,
    /// Whether it's restricted, so its preview waits until it's asked for.
    pub restricted: bool,
}

#[uniffi::export]
impl ArchiveHandle {
    /// What the question `id` is about: the filed items it names, then the waiting proposals
    /// its answer settles.
    ///
    /// # Errors
    ///
    /// [`CarrelError::Failed`] if the question isn't found or a queue can't be read.
    pub fn question_subjects(&self, id: String) -> Result<Vec<QuestionSubject>, CarrelError> {
        let archive = self.fresh()?;
        let question = QuestionQueue::of(&archive).find(&id)?;
        Ok(subjects(&archive, &question)?
            .into_iter()
            .map(|s| QuestionSubject {
                id: s.id.to_string(),
                proposal: s.proposal,
                title: s.title,
                summary: s.summary,
                card: s.card,
                file: s.path.as_deref().map(text),
                kind: s.kind.into(),
                restricted: s.restricted,
            })
            .collect())
    }
}
